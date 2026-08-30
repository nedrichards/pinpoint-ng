use futures_lite::StreamExt;
use gst::prelude::*;
use gtk::prelude::*;
use gtk::{gdk, gio, glib, pango};
use pinpoint_core::presentation::{BackgroundType, Presentation, Slide, TextAlign};
use pinpoint_core::render::{background_rect, shading_rect, text_rect};
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{IsTerminal, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

const MAX_PDF_SVG_BYTES: u64 = 16 * 1024 * 1024;
const MAX_EMBEDDED_JPEG_BYTES: u64 = 64 * 1024 * 1024;

const A4_SHORT: f64 = 595.275_590_551;
const A4_LONG: f64 = 841.889_763_78;
const LETTER_SHORT: f64 = 612.0;
const LETTER_LONG: f64 = 792.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PageSize {
    #[default]
    A4,
    Letter,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    #[default]
    Landscape,
    Portrait,
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub page_size: PageSize,
    pub orientation: Orientation,
    pub include_speaker_notes: bool,
    pub asset_access: pinpoint_core::asset::Access,
}

#[derive(Debug)]
struct ExportStats {
    slides: usize,
    pages: usize,
    jpeg_sources: usize,
    raster_peak_bytes: u64,
    svg_documents: usize,
    video_thumbnails: usize,
    video_failures: usize,
}

struct TemporaryOutput {
    path: PathBuf,
    committed: bool,
}

impl Drop for TemporaryOutput {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

struct RasterCache {
    path: PathBuf,
    surface: cairo::ImageSurface,
    intrinsic_width: i32,
    intrinsic_height: i32,
    pixel_width: i32,
    pixel_height: i32,
    bytes: u64,
    jpeg: bool,
}

fn page_dimensions(options: Options) -> (f64, f64) {
    let (short, long) = match options.page_size {
        PageSize::A4 => (A4_SHORT, A4_LONG),
        PageSize::Letter => (LETTER_SHORT, LETTER_LONG),
    };
    match options.orientation {
        Orientation::Landscape => (long, short),
        Orientation::Portrait => (short, long),
    }
}

fn output_identity(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return path
            .canonicalize()
            .map_err(|error| format!("cannot resolve {}: {error}", path.display()));
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent = parent
        .canonicalize()
        .map_err(|error| format!("cannot resolve output folder {}: {error}", parent.display()))?;
    let filename = path
        .file_name()
        .ok_or_else(|| "PDF output must name a local file".to_owned())?;
    Ok(parent.join(filename))
}

fn reject_source_destination(source: &Path, output: &Path) -> Result<(), String> {
    let source_identity = source
        .canonicalize()
        .map_err(|error| format!("cannot resolve {}: {error}", source.display()))?;
    let output_identity = output_identity(output)?;
    if source_identity == output_identity {
        return Err("PDF output must not replace the presentation source".into());
    }
    if let (Ok(source_metadata), Ok(output_metadata)) =
        (std::fs::metadata(source), std::fs::metadata(output))
        && source_metadata.dev() == output_metadata.dev()
        && source_metadata.ino() == output_metadata.ino()
    {
        return Err("PDF output must not replace the presentation source".into());
    }
    Ok(())
}

fn create_temporary(output: &Path) -> Result<(TemporaryOutput, File), String> {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    let filename = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "PDF output must have a UTF-8 local filename".to_owned())?;
    for serial in 0..1_024_u32 {
        let path = parent.join(format!(
            ".pinpoint-pdf-{}-{serial:08x}-{filename}",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o666)
            .open(&path)
        {
            Ok(file) => {
                return Ok((
                    TemporaryOutput {
                        path,
                        committed: false,
                    },
                    file,
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("cannot create temporary PDF: {error}")),
        }
    }
    Err("cannot allocate a unique temporary PDF filename".into())
}

fn resolve_asset(
    source: &Path,
    asset: &str,
    access: pinpoint_core::asset::Access,
) -> Result<PathBuf, String> {
    pinpoint_core::asset::resolve_local(Some(source), asset, access)
        .ok_or_else(|| format!("unsupported non-local PDF asset: {asset}"))
}

fn rgba(value: &str, fallback: &str) -> gdk::RGBA {
    gdk::RGBA::parse(value)
        .or_else(|_| gdk::RGBA::parse(fallback))
        .expect("built-in PDF fallback colour is valid")
}

fn set_source_rgba(context: &cairo::Context, color: gdk::RGBA, alpha: f64) {
    context.set_source_rgba(
        color.red().into(),
        color.green().into(),
        color.blue().into(),
        f64::from(color.alpha()) * alpha,
    );
}

fn paint_color(context: &cairo::Context, value: &str, fallback: &str) -> Result<(), String> {
    set_source_rgba(context, rgba(value, fallback), 1.0);
    context.paint().map_err(|error| error.to_string())
}

fn rgba_surface(
    width: i32,
    height: i32,
    source: &[u8],
    source_stride: usize,
) -> Result<cairo::ImageSurface, String> {
    if width <= 0 || height <= 0 {
        return Err("decoded frame has invalid dimensions".into());
    }
    let row_bytes = width as usize * 4;
    if source_stride < row_bytes || source.len() < source_stride.saturating_mul(height as usize) {
        return Err("decoded frame has an unsafe RGBA layout".into());
    }
    let stride = cairo::Format::ARgb32
        .stride_for_width(width as u32)
        .map_err(|error| error.to_string())?;
    let mut pixels = vec![0_u8; stride as usize * height as usize];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let src = y * source_stride + x * 4;
            let dst = y * stride as usize + x * 4;
            #[cfg(target_endian = "little")]
            pixels[dst..dst + 4].copy_from_slice(&[
                source[src + 2],
                source[src + 1],
                source[src],
                source[src + 3],
            ]);
            #[cfg(target_endian = "big")]
            pixels[dst..dst + 4].copy_from_slice(&[
                source[src + 3],
                source[src],
                source[src + 1],
                source[src + 2],
            ]);
        }
    }
    cairo::ImageSurface::create_for_data(pixels, cairo::Format::ARgb32, width, height, stride)
        .map_err(|error| error.to_string())
}

fn texture_surface(texture: &gdk::Texture) -> Result<cairo::ImageSurface, String> {
    let width = texture.width();
    let height = texture.height();
    let mut downloader = gdk::TextureDownloader::new(texture);
    downloader.set_format(gdk::MemoryFormat::R8g8b8a8Premultiplied);
    let (bytes, source_stride) = downloader.download_bytes();
    rgba_surface(width, height, bytes.as_ref(), source_stride)
}

fn pull_video_preroll(
    sink: &gst::Element,
    cancellable: &gio::Cancellable,
) -> Result<Option<gst::Sample>, String> {
    for _ in 0..10 {
        if cancellable.is_cancelled() {
            return Err("PDF export cancelled".into());
        }
        let sample = sink.emit_by_name::<Option<gst::Sample>>(
            "try-pull-preroll",
            &[&gst::ClockTime::from_mseconds(100)],
        );
        if sample.is_some() {
            return Ok(sample);
        }
    }
    Ok(None)
}

fn score_video_sample(sample: &gst::Sample) -> Option<(f64, bool)> {
    let caps = sample.caps()?.structure(0)?;
    if caps.get::<&str>("format").ok()? != "RGBA" {
        return None;
    }
    let width = caps.get::<i32>("width").ok()?;
    let height = caps.get::<i32>("height").ok()?;
    if width <= 0 || height <= 0 {
        return None;
    }
    let buffer = sample.buffer()?;
    let map = buffer.map_readable().ok()?;
    if map.len() % height as usize != 0 {
        return None;
    }
    let stride = map.len() / height as usize;
    let row_bytes = width as usize * 4;
    if stride < row_bytes {
        return None;
    }

    let x_step = (width as usize / 96).max(1);
    let y_step = (height as usize / 54).max(1);
    let mut sum = 0.0;
    let mut squared_sum = 0.0;
    let mut edge_sum = 0.0;
    let mut samples = 0_u32;
    let mut edges = 0_u32;
    let luminance = |offset: usize| {
        f64::from(map[offset]) * 0.2126
            + f64::from(map[offset + 1]) * 0.7152
            + f64::from(map[offset + 2]) * 0.0722
    };
    for y in (0..height as usize).step_by(y_step) {
        for x in (0..width as usize).step_by(x_step) {
            let offset = y * stride + x * 4;
            let value = luminance(offset);
            sum += value;
            squared_sum += value * value;
            samples += 1;
            if x >= x_step {
                edge_sum += (value - luminance(offset - x_step * 4)).abs();
                edges += 1;
            }
            if y >= y_step {
                edge_sum += (value - luminance(offset - y_step * stride)).abs();
                edges += 1;
            }
        }
    }
    if samples == 0 {
        return None;
    }
    let mean = sum / f64::from(samples);
    let deviation = (squared_sum / f64::from(samples) - mean * mean)
        .max(0.0)
        .sqrt();
    let detail = if edges == 0 {
        0.0
    } else {
        edge_sum / f64::from(edges)
    };
    let exposure = 1.0 - (mean - 127.5).abs() / 127.5;
    let acceptable = !((mean < 18.0 && deviation < 18.0) || (deviation < 7.0 && detail < 6.0));
    Some((deviation * 4.0 + detail * 3.0 + exposure * 25.0, acceptable))
}

fn video_raster_from_sample(
    sample: &gst::Sample,
    path: &Path,
    max_width: i32,
    max_height: i32,
) -> Result<RasterCache, String> {
    let caps = sample
        .caps()
        .and_then(|caps| caps.structure(0))
        .ok_or_else(|| "video thumbnail has no caps".to_owned())?;
    let format = caps
        .get::<&str>("format")
        .map_err(|_| "video thumbnail has no pixel format".to_owned())?;
    if format != "RGBA" {
        return Err("video thumbnail did not negotiate RGBA".into());
    }
    let width = caps
        .get::<i32>("width")
        .map_err(|_| "video thumbnail has no width".to_owned())?;
    let height = caps
        .get::<i32>("height")
        .map_err(|_| "video thumbnail has no height".to_owned())?;
    let buffer = sample
        .buffer()
        .ok_or_else(|| "video thumbnail has no frame buffer".to_owned())?;
    let map = buffer
        .map_readable()
        .map_err(|error| format!("cannot map video thumbnail: {error}"))?;
    if height <= 0 || map.len() % height as usize != 0 {
        return Err("video thumbnail has an unsafe RGBA layout".into());
    }
    let source_stride = map.len() / height as usize;
    let source_surface = rgba_surface(width, height, map.as_ref(), source_stride)?;
    let (pixel_width, pixel_height) = scaled_video_dimensions(width, height, max_width, max_height);
    let surface = if pixel_width == width && pixel_height == height {
        source_surface
    } else {
        let scaled = cairo::ImageSurface::create(cairo::Format::ARgb32, pixel_width, pixel_height)
            .map_err(|error| error.to_string())?;
        let context = cairo::Context::new(&scaled).map_err(|error| error.to_string())?;
        context.scale(
            f64::from(pixel_width) / f64::from(width),
            f64::from(pixel_height) / f64::from(height),
        );
        context
            .set_source_surface(&source_surface, 0.0, 0.0)
            .map_err(|error| error.to_string())?;
        context.paint().map_err(|error| error.to_string())?;
        scaled
    };
    let identity = format!(
        "video:{}x{}:{}",
        width,
        height,
        path.canonicalize()
            .unwrap_or_else(|_| path.to_owned())
            .display()
    );
    surface
        .set_mime_data(cairo::MIME_TYPE_UNIQUE_ID, identity.into_bytes())
        .map_err(|error| error.to_string())?;
    Ok(RasterCache {
        path: path.to_owned(),
        surface,
        intrinsic_width: width,
        intrinsic_height: height,
        pixel_width,
        pixel_height,
        bytes: (pixel_width as u64)
            .saturating_mul(pixel_height as u64)
            .saturating_mul(4),
        jpeg: false,
    })
}

fn scaled_video_dimensions(width: i32, height: i32, max_width: i32, max_height: i32) -> (i32, i32) {
    if width <= max_width && height <= max_height {
        return (width, height);
    }
    let scale =
        (f64::from(max_width) / f64::from(width)).min(f64::from(max_height) / f64::from(height));
    (
        (f64::from(width) * scale).floor().max(1.0) as i32,
        (f64::from(height) * scale).floor().max(1.0) as i32,
    )
}

fn load_video_thumbnail(
    path: &Path,
    cancellable: &gio::Cancellable,
    max_width: i32,
    max_height: i32,
) -> Result<RasterCache, String> {
    if cancellable.is_cancelled() {
        return Err("PDF export cancelled".into());
    }
    gst::init().map_err(|error| error.to_string())?;
    let player = gst::ElementFactory::make("playbin3")
        .build()
        .or_else(|_| gst::ElementFactory::make("playbin").build())
        .map_err(|error| format!("cannot create video thumbnail player: {error}"))?;
    let sink = gst::ElementFactory::make("appsink")
        .property(
            "caps",
            gst::Caps::builder("video/x-raw")
                .field("format", "RGBA")
                .build(),
        )
        .property("max-buffers", 1_u32)
        .property("drop", true)
        .property("sync", false)
        .build()
        .map_err(|error| format!("cannot create video thumbnail sink: {error}"))?;
    let audio_sink = gst::ElementFactory::make("fakesink")
        .property("sync", false)
        .build()
        .map_err(|error| format!("cannot create video thumbnail audio sink: {error}"))?;
    player.set_property("uri", gio::File::for_path(path).uri());
    player.set_property("video-sink", &sink);
    player.set_property("audio-sink", &audio_sink);

    let result = (|| {
        player
            .set_state(gst::State::Paused)
            .map_err(|error| format!("cannot prepare video thumbnail: {error}"))?;
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            if cancellable.is_cancelled() {
                return Err("PDF export cancelled".into());
            }
            if player
                .state(Some(gst::ClockTime::from_mseconds(100)))
                .0
                .is_ok()
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err("timed out while decoding a video thumbnail".into());
            }
        }

        let mut best = None;
        let mut fallback = None;
        if let Some(duration) = player.query_duration::<gst::ClockTime>()
            && duration > gst::ClockTime::from_seconds(1)
        {
            for fraction in [0.12, 0.38, 0.63, 0.86] {
                if cancellable.is_cancelled() {
                    return Err("PDF export cancelled".into());
                }
                let nanoseconds = ((duration.nseconds() as f64 * fraction) as u64)
                    .clamp(250_000_000, duration.nseconds().saturating_sub(100_000_000));
                if player
                    .seek_simple(
                        gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                        gst::ClockTime::from_nseconds(nanoseconds),
                    )
                    .is_ok()
                    && let Some(sample) = pull_video_preroll(&sink, cancellable)?
                    && let Some((score, acceptable)) = score_video_sample(&sample)
                {
                    if fallback
                        .as_ref()
                        .is_none_or(|(best_score, _)| score > *best_score)
                    {
                        fallback = Some((score, sample.clone()));
                    }
                    if acceptable
                        && best
                            .as_ref()
                            .is_none_or(|(best_score, _)| score > *best_score)
                    {
                        best = Some((score, sample));
                    }
                }
            }
        }
        let sample = match best.or(fallback) {
            Some((_, sample)) => sample,
            None => pull_video_preroll(&sink, cancellable)?
                .ok_or_else(|| "timed out while decoding a video thumbnail".to_owned())?,
        };
        video_raster_from_sample(&sample, path, max_width, max_height)
    })();
    let _ = player.set_state(gst::State::Null);
    result
}

async fn load_raster(path: &Path, cancellable: &gio::Cancellable) -> Result<RasterCache, String> {
    if cancellable.is_cancelled() {
        return Err("PDF export cancelled".into());
    }
    let mut loader = glycin::Loader::new(gio::File::for_path(path));
    loader.cancellable(cancellable.clone());
    let image = loader
        .load()
        .await
        .map_err(|error| format!("cannot decode {}: {error}", path.display()))?;
    let details = image.details();
    let intrinsic_width = details.width().max(1);
    let intrinsic_height = details.height().max(1);
    let max_width = A4_LONG.mul_add(2.0, 0.0).ceil() as u32;
    let max_height = A4_LONG.mul_add(2.0, 0.0).ceil() as u32;
    let scale = (max_width as f64 / intrinsic_width as f64)
        .min(max_height as f64 / intrinsic_height as f64)
        .min(1.0);
    let requested_width = (intrinsic_width as f64 * scale).round().max(1.0) as u32;
    let requested_height = (intrinsic_height as f64 * scale).round().max(1.0) as u32;
    let frame = image
        .specific_frame(glycin::FrameRequest::new().scale(requested_width, requested_height))
        .await
        .map_err(|error| format!("cannot decode {}: {error}", path.display()))?;
    let texture = frame.texture();
    let width = texture.width();
    let height = texture.height();
    let surface = texture_surface(&texture)?;
    let mut prefix = [0_u8; 3];
    let mut input =
        File::open(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let jpeg = input.read_exact(&mut prefix).is_ok() && prefix == [0xff, 0xd8, 0xff];
    let jpeg_passthrough = jpeg
        && width as u32 == intrinsic_width
        && height as u32 == intrinsic_height
        && std::fs::metadata(path).is_ok_and(|metadata| metadata.len() <= MAX_EMBEDDED_JPEG_BYTES);
    drop(input);
    if jpeg_passthrough {
        let encoded = std::fs::read(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        surface
            .set_mime_data(cairo::MIME_TYPE_JPEG, encoded)
            .map_err(|error| error.to_string())?;
        let identity = format!(
            "jpeg:{}x{}:{}",
            width,
            height,
            path.canonicalize()
                .unwrap_or_else(|_| path.to_owned())
                .display()
        );
        surface
            .set_mime_data(cairo::MIME_TYPE_UNIQUE_ID, identity.into_bytes())
            .map_err(|error| error.to_string())?;
    }
    Ok(RasterCache {
        path: path.to_owned(),
        surface,
        intrinsic_width: intrinsic_width as i32,
        intrinsic_height: intrinsic_height as i32,
        pixel_width: width,
        pixel_height: height,
        bytes: (width as u64)
            .saturating_mul(height as u64)
            .saturating_mul(4),
        jpeg: jpeg_passthrough,
    })
}

fn draw_raster(
    context: &cairo::Context,
    slide: &Slide,
    raster: &RasterCache,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let rect = background_rect(
        slide,
        width as f32,
        height as f32,
        raster.intrinsic_width as f32,
        raster.intrinsic_height as f32,
    );
    context.save().map_err(|error| error.to_string())?;
    context.rectangle(0.0, 0.0, width, height);
    context.clip();
    context.translate(rect.x.into(), rect.y.into());
    context.scale(
        f64::from(rect.width) / f64::from(raster.pixel_width),
        f64::from(rect.height) / f64::from(raster.pixel_height),
    );
    context
        .set_source_surface(&raster.surface, 0.0, 0.0)
        .map_err(|error| error.to_string())?;
    context.paint().map_err(|error| error.to_string())?;
    context.restore().map_err(|error| error.to_string())
}

fn load_svg(path: &Path, cancellable: &gio::Cancellable) -> Result<rsvg::SvgHandle, String> {
    if std::fs::metadata(path)
        .map_err(|error| format!("cannot inspect SVG {}: {error}", path.display()))?
        .len()
        > MAX_PDF_SVG_BYTES
    {
        return Err(format!(
            "SVG {} exceeds the 16 MiB safety limit",
            path.display()
        ));
    }
    let file = gio::File::for_path(path);
    rsvg::Loader::new()
        .read_file(&file, Some(cancellable))
        .map_err(|error| format!("cannot load SVG {}: {error}", path.display()))
}

fn draw_svg(
    context: &cairo::Context,
    slide: &Slide,
    path: &Path,
    handle: &rsvg::SvgHandle,
    width: f64,
    height: f64,
    cancellable: &gio::Cancellable,
) -> Result<(), String> {
    let renderer = rsvg::CairoRenderer::new(handle).with_cancellable(cancellable);
    let (intrinsic_width, intrinsic_height) = renderer
        .intrinsic_size_in_pixels()
        .unwrap_or((width, height));
    let rect = background_rect(
        slide,
        width as f32,
        height as f32,
        intrinsic_width as f32,
        intrinsic_height as f32,
    );
    renderer
        .render_document(
            context,
            &cairo::Rectangle::new(
                rect.x.into(),
                rect.y.into(),
                rect.width.into(),
                rect.height.into(),
            ),
        )
        .map_err(|error| format!("cannot render SVG {}: {error}", path.display()))
}

fn draw_text(
    context: &cairo::Context,
    slide: &Slide,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let Some(text) = slide.text.as_deref().filter(|text| !text.is_empty()) else {
        return Ok(());
    };
    let layout = pangocairo::functions::create_layout(context);
    if slide.use_markup {
        layout.set_markup(text);
    } else {
        layout.set_text(text);
    }
    layout.set_font_description(Some(&pango::FontDescription::from_string(&slide.font)));
    layout.set_alignment(match slide.text_align {
        TextAlign::Left => pango::Alignment::Left,
        TextAlign::Center => pango::Alignment::Center,
        TextAlign::Right => pango::Alignment::Right,
    });
    let (_, logical) = layout.pixel_extents();
    let (text_rect, scale) = text_rect(
        slide,
        width as f32,
        height as f32,
        logical.width() as f32,
        logical.height() as f32,
    );
    let shade = shading_rect(width as f32, text_rect);
    set_source_rgba(
        context,
        rgba(&slide.shading_color, "black"),
        slide.shading_opacity.clamp(0.0, 1.0),
    );
    context.rectangle(
        shade.x.into(),
        shade.y.into(),
        shade.width.into(),
        shade.height.into(),
    );
    context.fill().map_err(|error| error.to_string())?;
    context.save().map_err(|error| error.to_string())?;
    context.translate(text_rect.x.into(), text_rect.y.into());
    context.scale(scale.into(), scale.into());
    set_source_rgba(context, rgba(&slide.text_color, "white"), 1.0);
    pangocairo::functions::show_layout(context, &layout);
    context.restore().map_err(|error| error.to_string())
}

fn draw_notes(
    context: &cairo::Context,
    slide: &Slide,
    width: f64,
    height: f64,
) -> Result<(), String> {
    paint_color(context, "white", "white")?;
    let notes = slide.speaker_notes.as_deref().unwrap_or_default();
    let layout = pangocairo::functions::create_layout(context);
    layout.set_text(notes);
    layout.set_font_description(Some(&pango::FontDescription::from_string(&format!(
        "{} {}",
        slide.notes_font, slide.notes_font_size
    ))));
    layout.set_width((width * 0.9 * f64::from(pango::SCALE)) as i32);
    layout.set_wrap(pango::WrapMode::WordChar);
    context.move_to(width * 0.05, height * 0.05);
    set_source_rgba(context, rgba("black", "black"), 1.0);
    pangocairo::functions::show_layout(context, &layout);
    Ok(())
}

async fn export<F>(
    source: &Path,
    output: &Path,
    presentation: &Presentation,
    options: Options,
    cancellable: &gio::Cancellable,
    mut progress: F,
) -> Result<ExportStats, String>
where
    F: FnMut(usize, usize),
{
    reject_source_destination(source, output)?;
    let output_snapshot = pinpoint_core::atomic_replace::snapshot(output)
        .map_err(|error| format!("cannot inspect {}: {error}", output.display()))?;
    let (mut temporary, file) = create_temporary(output)?;
    let (width, height) = page_dimensions(options);
    let surface = cairo::PdfSurface::for_stream(width, height, file)
        .map_err(|error| format!("cannot create PDF: {error}"))?;
    let mut raster_cache: Option<RasterCache> = None;
    let mut svg_cache = HashMap::<PathBuf, rsvg::SvgHandle>::new();
    let mut failed_videos = HashSet::<PathBuf>::new();
    let mut stats = ExportStats {
        slides: presentation.slides.len(),
        pages: 0,
        jpeg_sources: 0,
        raster_peak_bytes: 0,
        svg_documents: 0,
        video_thumbnails: 0,
        video_failures: 0,
    };
    for (index, slide) in presentation.slides.iter().enumerate() {
        if cancellable.is_cancelled() {
            return Err("PDF export cancelled".into());
        }
        surface
            .set_size(width, height)
            .map_err(|error| error.to_string())?;
        let context = cairo::Context::new(&surface).map_err(|error| error.to_string())?;
        paint_color(&context, "white", "white")?;
        paint_color(&context, &slide.stage_color, "black")?;
        if let Some(background) = slide.background.as_deref() {
            match slide.background_type {
                BackgroundType::Color => paint_color(&context, background, "black")?,
                BackgroundType::Image => {
                    let path = resolve_asset(source, background, options.asset_access)?;
                    if raster_cache.as_ref().is_none_or(|cache| cache.path != path) {
                        raster_cache = Some(load_raster(&path, cancellable).await?);
                    }
                    let raster = raster_cache.as_ref().unwrap();
                    stats.raster_peak_bytes = stats.raster_peak_bytes.max(raster.bytes);
                    if raster.jpeg {
                        stats.jpeg_sources += 1;
                    }
                    draw_raster(&context, slide, raster, width, height)?;
                }
                BackgroundType::Svg => {
                    let path = resolve_asset(source, background, options.asset_access)?;
                    if !svg_cache.contains_key(&path) {
                        if svg_cache.len() >= 8
                            && let Some(oldest) = svg_cache.keys().next().cloned()
                        {
                            svg_cache.remove(&oldest);
                        }
                        svg_cache.insert(path.clone(), load_svg(&path, cancellable)?);
                    }
                    draw_svg(
                        &context,
                        slide,
                        &path,
                        svg_cache.get(&path).unwrap(),
                        width,
                        height,
                        cancellable,
                    )?;
                    stats.svg_documents += 1;
                }
                BackgroundType::Video => {
                    let path = resolve_asset(source, background, options.asset_access)?;
                    if !failed_videos.contains(&path) {
                        if raster_cache.as_ref().is_none_or(|cache| cache.path != path) {
                            match load_video_thumbnail(
                                &path,
                                cancellable,
                                (width * 2.0).ceil() as i32,
                                (height * 2.0).ceil() as i32,
                            ) {
                                Ok(raster) => raster_cache = Some(raster),
                                Err(error) if error == "PDF export cancelled" => return Err(error),
                                Err(error) => {
                                    eprintln!(
                                        "pinpoint: unable to create PDF video thumbnail for {}: {error}",
                                        path.display()
                                    );
                                    failed_videos.insert(path.clone());
                                    stats.video_failures += 1;
                                }
                            }
                        }
                        if let Some(raster) =
                            raster_cache.as_ref().filter(|cache| cache.path == path)
                        {
                            stats.raster_peak_bytes = stats.raster_peak_bytes.max(raster.bytes);
                            draw_raster(&context, slide, raster, width, height)?;
                            stats.video_thumbnails += 1;
                        }
                    }
                }
                BackgroundType::Camera | BackgroundType::None => {}
            }
        }
        draw_text(&context, slide, width, height)?;
        context.show_page().map_err(|error| error.to_string())?;
        stats.pages += 1;
        if options.include_speaker_notes && slide.speaker_notes.is_some() {
            if cancellable.is_cancelled() {
                return Err("PDF export cancelled".into());
            }
            surface
                .set_size(width, height)
                .map_err(|error| error.to_string())?;
            let notes_context = cairo::Context::new(&surface).map_err(|error| error.to_string())?;
            draw_notes(&notes_context, slide, width, height)?;
            notes_context
                .show_page()
                .map_err(|error| error.to_string())?;
            stats.pages += 1;
        }
        progress(index + 1, presentation.slides.len());
        if std::io::stderr().is_terminal() {
            eprintln!(
                "Exported slide {} of {}",
                index + 1,
                presentation.slides.len()
            );
        }
    }
    if cancellable.is_cancelled() {
        return Err("PDF export cancelled".into());
    }
    let stream = surface
        .finish_output_stream()
        .map_err(|error| format!("cannot finish PDF: {error}"))?;
    let file = stream
        .downcast::<File>()
        .map_err(|_| "PDF output stream changed type unexpectedly".to_owned())?;
    file.sync_all()
        .map_err(|error| format!("cannot flush PDF: {error}"))?;
    if cancellable.is_cancelled() {
        return Err("PDF export cancelled".into());
    }
    pinpoint_core::atomic_replace::commit_if_unchanged(&temporary.path, output, output_snapshot)
        .map_err(|error| format!("cannot replace {}: {error}", output.display()))?;
    if let Some(parent) = output.parent()
        && let Ok(directory) = File::open(parent)
    {
        let _ = directory.sync_all();
    }
    temporary.committed = true;
    Ok(stats)
}

fn install_signal_cancellation(cancellable: gio::Cancellable) -> Result<Arc<AtomicI32>, String> {
    let signal_number = Arc::new(AtomicI32::new(0));
    let state = signal_number.clone();
    let mut signals =
        async_signal::Signals::new([async_signal::Signal::Int, async_signal::Signal::Term])
            .map_err(|error| format!("cannot install signal handlers: {error}"))?;
    std::thread::spawn(move || {
        futures_lite::future::block_on(async move {
            while let Some(Ok(signal)) = signals.next().await {
                let number = signal as i32;
                if state
                    .compare_exchange(0, number, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    cancellable.cancel();
                } else {
                    std::process::exit(128 + number);
                }
            }
        });
    });
    Ok(signal_number)
}

pub fn run(
    source: &Path,
    output: &Path,
    ignore_comments: bool,
    options: Options,
) -> glib::ExitCode {
    if let Err(error) = gtk::init() {
        eprintln!("pinpoint: cannot initialize GTK for PDF rendering: {error}");
        return glib::ExitCode::FAILURE;
    }
    let presentation = match pinpoint_core::presentation::load_for_pdf(source, ignore_comments) {
        Ok(presentation) => presentation,
        Err(error) => {
            eprintln!("pinpoint: {error}");
            return glib::ExitCode::FAILURE;
        }
    };
    let cancellable = gio::Cancellable::new();
    let signal_number = match install_signal_cancellation(cancellable.clone()) {
        Ok(signal_number) => signal_number,
        Err(error) => {
            eprintln!("pinpoint: {error}");
            return glib::ExitCode::FAILURE;
        }
    };
    let result = glib::MainContext::default().block_on(export(
        source,
        output,
        &presentation,
        options,
        &cancellable,
        |_, _| {},
    ));
    let interrupted = signal_number.load(Ordering::SeqCst);
    if interrupted != 0 {
        return glib::ExitCode::from((128 + interrupted) as u8);
    }
    match result {
        Ok(stats) => {
            eprintln!(
                "PINPOINT PDF PASS slides={} pages={} jpeg_uses={} svg_uses={} video_thumbnails={} video_failures={} raster_peak_mib={:.2} output={}",
                stats.slides,
                stats.pages,
                stats.jpeg_sources,
                stats.svg_documents,
                stats.video_thumbnails,
                stats.video_failures,
                stats.raster_peak_bytes as f64 / 1024.0 / 1024.0,
                output.display()
            );
            glib::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("pinpoint: {error}");
            glib::ExitCode::FAILURE
        }
    }
}

pub fn export_async(
    source: PathBuf,
    output: PathBuf,
    ignore_comments: bool,
    options: Options,
    cancellable: gio::Cancellable,
    progress: impl FnMut(usize, usize) + 'static,
    callback: impl FnOnce(Result<(), String>) + 'static,
) {
    glib::MainContext::default().spawn_local(async move {
        let result = match pinpoint_core::presentation::load_for_pdf(&source, ignore_comments) {
            Ok(presentation) => export(
                &source,
                &output,
                &presentation,
                options,
                &cancellable,
                progress,
            )
            .await
            .map(|_| ()),
            Err(error) => Err(error.to_string()),
        };
        callback(result);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_geometry_matches_the_c_exporter() {
        assert_eq!(
            page_dimensions(Options {
                page_size: PageSize::A4,
                orientation: Orientation::Landscape,
                include_speaker_notes: true,
                asset_access: pinpoint_core::asset::Access::Confined,
            }),
            (A4_LONG, A4_SHORT)
        );
        assert_eq!(
            page_dimensions(Options {
                page_size: PageSize::Letter,
                orientation: Orientation::Portrait,
                include_speaker_notes: false,
                asset_access: pinpoint_core::asset::Access::Confined,
            }),
            (LETTER_SHORT, LETTER_LONG)
        );
    }

    #[test]
    fn source_and_hardlink_destinations_are_rejected() {
        let directory =
            std::env::temp_dir().join(format!("pinpoint-pdf-identity-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("talk.pin");
        let alias = directory.join("alias.pin");
        std::fs::write(&source, "--\nslide\n").unwrap();
        let _ = std::fs::remove_file(&alias);
        std::fs::hard_link(&source, &alias).unwrap();
        assert!(reject_source_destination(&source, &source).is_err());
        assert!(reject_source_destination(&source, &alias).is_err());
        std::fs::remove_file(alias).unwrap();
        std::fs::remove_file(source).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn external_pdf_destination_change_wins_the_commit_race() {
        let directory =
            std::env::temp_dir().join(format!("pinpoint-pdf-race-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let output = directory.join("slides.pdf");
        let temporary = directory.join("generated.tmp");
        std::fs::write(&output, "old output").unwrap();
        let snapshot = pinpoint_core::atomic_replace::snapshot(&output).unwrap();
        std::fs::write(&temporary, "%PDF-generated").unwrap();
        std::fs::write(&output, "external output").unwrap();
        assert!(matches!(
            pinpoint_core::atomic_replace::commit_if_unchanged(&temporary, &output, snapshot),
            Err(pinpoint_core::atomic_replace::Error::Changed)
        ));
        assert_eq!(std::fs::read_to_string(&output).unwrap(), "external output");
        std::fs::remove_file(temporary).unwrap();
        std::fs::remove_file(output).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn video_thumbnails_are_bounded_to_pdf_resolution() {
        assert_eq!(scaled_video_dimensions(1280, 720, 1684, 1191), (1280, 720));
        assert_eq!(scaled_video_dimensions(3840, 2160, 1684, 1191), (1684, 947));
        assert_eq!(scaled_video_dimensions(2160, 3840, 1684, 1191), (669, 1191));
    }
}
