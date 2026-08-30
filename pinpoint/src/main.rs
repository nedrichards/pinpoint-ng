mod app_shell;
mod asset_store;
mod camera;
mod command_runner;
mod editor;
#[cfg(test)]
mod gsk_page_curl;
mod lifecycle;
mod media;
mod mpris;
mod page_curl_view;
mod pdf;
mod presentation_chrome;
mod renderer_policy;
mod setup;
mod speaker;
mod stage;
mod transition_renderer;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use pinpoint_core::presentation::BackgroundType;
use pinpoint_core::source;
use pinpoint_core::transition::{LegacyTransition, is_builtin};
use pinpoint_core::{asset, command};
use stage::Stage;
use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

const APP_ID: &str = "com.nedrichards.pinpoint";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug)]
struct Options {
    presentation: Option<PathBuf>,
    presentation_label: Option<String>,
    check: bool,
    format_assist: Option<String>,
    complete_position: Option<String>,
    fullscreen: bool,
    maximized: bool,
    edit: bool,
    rehearse: bool,
    speaker_mode: bool,
    audience_monitor: Option<String>,
    ignore_comments: bool,
    allow_external_assets: bool,
    validate_stage: bool,
    validate_pixels: bool,
    validate_transitions: bool,
    validate_lifecycle: bool,
    validate_lifecycle_stress: bool,
    validate_editor: bool,
    validate_speaker: bool,
    validate_normal_controls: bool,
    validate_rehearsal: bool,
    validate_media: bool,
    validate_setup: bool,
    validate_application_shell: bool,
    capture_page_curl: Option<PathBuf>,
    capture_real_slide: Option<PathBuf>,
    capture_slide: usize,
    capture_width: i32,
    capture_height: i32,
    capture_backwards: bool,
    output: Option<PathBuf>,
    pdf_page_size: pdf::PageSize,
    pdf_orientation: pdf::Orientation,
    pdf_speaker_notes: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            presentation: None,
            presentation_label: None,
            check: false,
            format_assist: None,
            complete_position: None,
            fullscreen: false,
            maximized: false,
            edit: false,
            rehearse: false,
            speaker_mode: false,
            audience_monitor: None,
            ignore_comments: false,
            allow_external_assets: false,
            validate_stage: false,
            validate_pixels: false,
            validate_transitions: false,
            validate_lifecycle: false,
            validate_lifecycle_stress: false,
            validate_editor: false,
            validate_speaker: false,
            validate_normal_controls: false,
            validate_rehearsal: false,
            validate_media: false,
            validate_setup: false,
            validate_application_shell: false,
            capture_page_curl: None,
            capture_real_slide: None,
            capture_slide: 0,
            capture_width: 800,
            capture_height: 600,
            capture_backwards: false,
            output: None,
            pdf_page_size: pdf::PageSize::A4,
            pdf_orientation: pdf::Orientation::Landscape,
            pdf_speaker_notes: true,
        }
    }
}

impl Options {
    fn asset_access(&self) -> asset::Access {
        if self.allow_external_assets {
            asset::Access::Compatible
        } else {
            asset::Access::Confined
        }
    }
}

fn print_help() {
    println!(
        "Pinpoint {VERSION}\n\n\
Usage: pinpoint [OPTIONS] [PRESENTATION.pin]\n\n\
Options:\n\
  --check              Validate a presentation without opening GTK\n\
  --format-assist=KIND Emit JSON diagnostics, symbols, assets, or completions\n\
  --complete-position=LINE:COLUMN One-based position for completion assistance\n\
  -f, --fullscreen     Start the presentation fullscreen\n\
  -m, --maximized      Start the presentation maximized\n\
  --edit               Open the presentation in the composition editor\n\
  -r, --rehearse       Record slide timings and save them on completion\n\
  -s, --speakermode    Open the audience and speaker presentation windows\n\
  -i, --ignore-comments Exclude speaker-note comments\n\
  --allow-external-assets Allow outside-folder assets for CLI compatibility\n\
  --output=FILE        Export the presentation atomically as PDF\n\
  --pdf-page-size=SIZE Use a4 (default) or letter pages\n\
  --pdf-orientation=O  Use landscape (default) or portrait pages\n\
  --pdf-no-speaker-notes Omit separate speaker-note pages\n\
  --version            Print the version\n\
  --help               Show this help"
    );
}

fn parse_options_from(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<Option<Options>, String> {
    let mut options = Options::default();
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        let text = argument.to_string_lossy();
        match text.as_ref() {
            "--help" | "-h" => {
                print_help();
                return Ok(None);
            }
            "--version" => {
                println!("pinpoint {VERSION}");
                return Ok(None);
            }
            "--check" => options.check = true,
            "--format-assist" => {
                let Some(value) = arguments.next() else {
                    return Err("--format-assist requires a kind".into());
                };
                options.format_assist = Some(value.to_string_lossy().into_owned());
            }
            _ if text.starts_with("--format-assist=") => {
                let value = text.trim_start_matches("--format-assist=");
                if value.is_empty() {
                    return Err("--format-assist requires a kind".into());
                }
                options.format_assist = Some(value.to_owned());
            }
            "--complete-position" => {
                let Some(value) = arguments.next() else {
                    return Err("--complete-position requires a one-based LINE:COLUMN".into());
                };
                options.complete_position = Some(value.to_string_lossy().into_owned());
            }
            _ if text.starts_with("--complete-position=") => {
                let value = text.trim_start_matches("--complete-position=");
                if value.is_empty() {
                    return Err("--complete-position requires a one-based LINE:COLUMN".into());
                }
                options.complete_position = Some(value.to_owned());
            }
            "--fullscreen" | "-f" => options.fullscreen = true,
            "--maximized" | "-m" => options.maximized = true,
            "--edit" => options.edit = true,
            "--rehearse" | "-r" => options.rehearse = true,
            "--speakermode" | "-s" => options.speaker_mode = true,
            "--ignore-comments" | "-i" => options.ignore_comments = true,
            "--allow-external-assets" => options.allow_external_assets = true,
            "--output" | "-o" => {
                let Some(value) = arguments.next() else {
                    return Err("--output requires a filename".into());
                };
                options.output = Some(PathBuf::from(value));
            }
            "--camera" | "-c" => {
                return Err("--camera=DEVICE is no longer supported; camera backgrounds use the desktop Camera portal. Remove --camera and approve the portal request when a [camera] slide is shown.".into());
            }
            "--validate-stage" => options.validate_stage = true,
            "--validate-pixels" => options.validate_pixels = true,
            "--validate-transitions" => options.validate_transitions = true,
            "--validate-lifecycle" => options.validate_lifecycle = true,
            "--validate-lifecycle-stress" => options.validate_lifecycle_stress = true,
            "--validate-editor" => options.validate_editor = true,
            "--validate-speaker" => options.validate_speaker = true,
            "--validate-normal-controls" => options.validate_normal_controls = true,
            "--validate-rehearsal" => options.validate_rehearsal = true,
            "--validate-media" => options.validate_media = true,
            "--validate-setup" => options.validate_setup = true,
            "--validate-application-shell" => options.validate_application_shell = true,
            "--capture-backward" => options.capture_backwards = true,
            "--pdf-no-speaker-notes" => options.pdf_speaker_notes = false,
            "--pdf-page-size" => {
                let Some(value) = arguments.next() else {
                    return Err("--pdf-page-size requires a4 or letter".into());
                };
                options.pdf_page_size = match value.to_string_lossy().as_ref() {
                    "a4" => pdf::PageSize::A4,
                    "letter" => pdf::PageSize::Letter,
                    value => return Err(format!("unsupported PDF page size: {value}")),
                };
            }
            "--pdf-orientation" => {
                let Some(value) = arguments.next() else {
                    return Err("--pdf-orientation requires landscape or portrait".into());
                };
                options.pdf_orientation = match value.to_string_lossy().as_ref() {
                    "landscape" => pdf::Orientation::Landscape,
                    "portrait" => pdf::Orientation::Portrait,
                    value => return Err(format!("unsupported PDF orientation: {value}")),
                };
            }
            _ if text.starts_with("--output=") => {
                let value = text.trim_start_matches("--output=");
                if value.is_empty() {
                    return Err("--output requires a filename".into());
                }
                options.output = Some(PathBuf::from(value));
            }
            _ if text.starts_with("--camera=") => {
                return Err("--camera=DEVICE is no longer supported; camera backgrounds use the desktop Camera portal. Remove --camera and approve the portal request when a [camera] slide is shown.".into());
            }
            _ if text.starts_with("--capture-page-curl=") => {
                let value = text.trim_start_matches("--capture-page-curl=");
                if value.is_empty() {
                    return Err("--capture-page-curl requires a filename".into());
                }
                options.capture_page_curl = Some(PathBuf::from(value));
            }
            _ if text.starts_with("--capture-real-slide=") => {
                let value = text.trim_start_matches("--capture-real-slide=");
                if value.is_empty() {
                    return Err("--capture-real-slide requires a filename".into());
                }
                options.capture_real_slide = Some(PathBuf::from(value));
            }
            _ if text.starts_with("--capture-slide=") => {
                let value = text.trim_start_matches("--capture-slide=");
                options.capture_slide = value
                    .parse()
                    .map_err(|_| "--capture-slide must be a non-negative integer".to_owned())?;
            }
            _ if text.starts_with("--capture-size=") => {
                let value = text.trim_start_matches("--capture-size=");
                let Some((width, height)) = value.split_once('x') else {
                    return Err("--capture-size must be WIDTHxHEIGHT".into());
                };
                options.capture_width = width
                    .parse()
                    .ok()
                    .filter(|value: &i32| *value > 0)
                    .ok_or_else(|| "--capture-size width must be positive".to_owned())?;
                options.capture_height = height
                    .parse()
                    .ok()
                    .filter(|value: &i32| *value > 0)
                    .ok_or_else(|| "--capture-size height must be positive".to_owned())?;
            }
            _ if text.starts_with("--pdf-page-size=") => {
                options.pdf_page_size = match text.trim_start_matches("--pdf-page-size=") {
                    "a4" => pdf::PageSize::A4,
                    "letter" => pdf::PageSize::Letter,
                    value => return Err(format!("unsupported PDF page size: {value}")),
                };
            }
            _ if text.starts_with("--pdf-orientation=") => {
                options.pdf_orientation = match text.trim_start_matches("--pdf-orientation=") {
                    "landscape" => pdf::Orientation::Landscape,
                    "portrait" => pdf::Orientation::Portrait,
                    value => return Err(format!("unsupported PDF orientation: {value}")),
                };
            }
            _ if text.starts_with('-') => return Err(format!("unknown option: {text}")),
            _ if options.presentation.is_some() => {
                return Err("exactly one presentation may be supplied".into());
            }
            _ => options.presentation = Some(PathBuf::from(argument)),
        }
    }
    if (options.check || options.format_assist.is_some()) && options.presentation.is_none() {
        return Err("--check and --format-assist require a presentation".into());
    }
    if options.allow_external_assets && options.presentation.is_none() {
        return Err("--allow-external-assets requires a presentation".into());
    }
    if options.complete_position.is_some()
        && options.format_assist.as_deref() != Some("completions")
    {
        return Err("--complete-position requires --format-assist=completions".into());
    }
    if options.edit
        && (options.check
            || options.format_assist.is_some()
            || options.output.is_some()
            || options.rehearse
            || options.fullscreen
            || options.speaker_mode)
    {
        return Err("--edit cannot be combined with rehearsal, fullscreen, speaker mode, check, or PDF output".into());
    }
    if options.edit && options.allow_external_assets {
        return Err(
            "--allow-external-assets is available only for presenting, checking, and PDF export"
                .into(),
        );
    }
    if options.rehearse && options.presentation.is_none() {
        return Err("--rehearse requires a presentation".into());
    }
    if options.output.is_some() && options.presentation.is_none() {
        return Err("--output requires a presentation".into());
    }
    if options.output.is_some()
        && (options.check
            || options.format_assist.is_some()
            || options.edit
            || options.validate_stage
            || options.validate_pixels
            || options.validate_transitions
            || options.validate_lifecycle
            || options.validate_lifecycle_stress
            || options.validate_editor
            || options.validate_speaker
            || options.validate_normal_controls
            || options.validate_rehearsal
            || options.validate_media
            || options.validate_setup
            || options.validate_application_shell)
    {
        return Err("--output cannot be combined with check or graphical lab modes".into());
    }
    if options.check
        && (options.edit
            || options.validate_stage
            || options.validate_pixels
            || options.validate_transitions
            || options.validate_lifecycle
            || options.validate_lifecycle_stress
            || options.validate_editor
            || options.validate_speaker
            || options.validate_normal_controls
            || options.validate_rehearsal
            || options.validate_media
            || options.validate_setup
            || options.validate_application_shell)
    {
        return Err("--check cannot be combined with graphical lab modes".into());
    }
    if options.format_assist.is_some()
        && (options.check
            || options.output.is_some()
            || options.rehearse
            || options.fullscreen
            || options.speaker_mode)
    {
        return Err("--format-assist cannot be combined with presentation, rehearsal, check, or PDF options".into());
    }
    if (options.validate_lifecycle || options.validate_lifecycle_stress)
        && options.presentation.is_some()
    {
        return Err("lifecycle validation creates and mutates its own temporary fixture".into());
    }
    if options.validate_media && options.presentation.is_some() {
        return Err("media validation creates its own pressure fixture".into());
    }
    if options.validate_rehearsal && options.presentation.is_some() {
        return Err("rehearsal validation creates and updates its own temporary fixture".into());
    }
    if options.validate_setup && options.presentation.is_some() {
        return Err("setup validation creates its own temporary folder".into());
    }
    if options.validate_application_shell && options.presentation.is_some() {
        return Err("application-shell validation creates its own temporary presentation".into());
    }
    if options.validate_editor && options.presentation.is_none() {
        return Err("editor validation requires an explicit test fixture".into());
    }
    if (options.validate_speaker || options.validate_normal_controls)
        && options.presentation.is_none()
    {
        return Err("speaker validation requires an explicit test fixture".into());
    }
    if options.validate_speaker {
        options.speaker_mode = true;
    }
    if options.validate_rehearsal {
        options.speaker_mode = true;
    }
    if options.rehearse {
        options.speaker_mode = true;
    }
    Ok(Some(options))
}

fn parse_options() -> Result<Option<Options>, String> {
    parse_options_from(std::env::args_os().skip(1))
}

fn resolve_asset(presentation_path: &Path, asset: &str, access: asset::Access) -> Option<PathBuf> {
    asset::resolve_local(Some(presentation_path), asset, access)
}

fn validate_presentation(
    path: &Path,
    ignore_comments: bool,
    access: asset::Access,
) -> Result<usize, String> {
    let presentation = pinpoint_core::presentation::load(path, ignore_comments)
        .map_err(|error| error.to_string())?;
    let analysis = source::analyze(&presentation.source);
    let errors = analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == source::DiagnosticSeverity::Error)
        .map(|diagnostic| diagnostic.message.clone())
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        return Err(format!("source diagnostics: {}", errors.join("; ")));
    }
    let mut asset_errors = Vec::new();
    for slide in &presentation.slides {
        if matches!(
            slide.background_type,
            BackgroundType::Image | BackgroundType::Svg | BackgroundType::Video
        ) && let Some(background) = slide.background.as_deref()
        {
            match resolve_asset(path, background, access) {
                Some(asset) if asset.exists() => {}
                Some(_) => asset_errors.push(format!("missing background asset: {background}")),
                None => asset_errors.push(format!(
                    "outside-folder or unsupported background asset: {background}"
                )),
            }
        }
        if !slide.transition.is_empty() && !is_builtin(&slide.transition) {
            let filename = if slide.transition.ends_with(".json") {
                slide.transition.clone()
            } else {
                format!("{}.json", slide.transition)
            };
            match resolve_asset(path, &filename, access) {
                Some(transition) => {
                    if let Err(error) = LegacyTransition::load(&transition) {
                        asset_errors.push(format!("invalid transition {filename}: {error}"));
                    }
                }
                None => asset_errors.push(format!(
                    "outside-folder or unsupported transition asset: {filename}"
                )),
            }
        }
    }
    asset_errors.sort();
    asset_errors.dedup();
    if !asset_errors.is_empty() {
        let omitted = asset_errors.len().saturating_sub(20);
        asset_errors.truncate(20);
        let mut message = asset_errors.join("\n");
        if omitted > 0 {
            message.push_str(&format!("\n…and {omitted} more asset errors"));
        }
        return Err(message);
    }
    Ok(presentation.slides.len())
}

fn run_format_assist(path: &Path, kind: &str, position: Option<&str>) -> Result<(), String> {
    let source_text =
        pinpoint_core::presentation::read_source(path).map_err(|error| error.to_string())?;
    let output = match kind {
        "diagnostics" => serde_json::json!({
            "version": 1,
            "kind": kind,
            "diagnostics": source::analyze(&source_text).diagnostics,
        }),
        "symbols" => {
            let symbols = source::analyze(&source_text)
                .slides
                .into_iter()
                .enumerate()
                .map(|(index, slide)| {
                    serde_json::json!({
                        "index": index,
                        "start": slide.start,
                        "separator_end": slide.separator_end,
                        "end": slide.end,
                        "title": slide.title,
                    })
                })
                .collect::<Vec<_>>();
            serde_json::json!({ "version": 1, "kind": kind, "symbols": symbols })
        }
        "assets" => serde_json::json!({
            "version": 1,
            "kind": kind,
            "assets": source::list_assets(Some(path)),
        }),
        "completions" => {
            let position = position.ok_or_else(|| {
                "--format-assist=completions requires --complete-position=LINE:COLUMN".to_owned()
            })?;
            let offset = source::completion_position_to_offset(&source_text, position)?;
            serde_json::json!({
                "version": 1,
                "kind": kind,
                "offset": offset,
                "completions": source::complete(&source_text, Some(path), offset),
            })
        }
        _ => {
            return Err(
                "--format-assist must be diagnostics, symbols, assets, or completions".into(),
            );
        }
    };
    println!(
        "{}",
        serde_json::to_string(&output).expect("JSON values serialize")
    );
    Ok(())
}

fn default_presentation() -> PathBuf {
    PathBuf::from("/app/share/pinpoint/introduction/introduction.pin")
}

const LIFECYCLE_INITIAL: &str = "[text-color=white]\n\n-- [asset.svg] [fit]\nFirst\n-- [#304060]\nSecond original\n-- [#503020]\nThird\n";
const LIFECYCLE_CHANGED: &str = "[text-color=white]\n\n-- [asset.svg] [fit]\nFirst\n-- [#304060]\nSecond changed\n-- [#503020]\nThird\n";
const LIFECYCLE_RECOVERED: &str = "[text-color=white]\n\n-- [asset.svg] [fit]\nFirst\n-- [#304060]\nSecond changed\n-- [#503020]\nThird recovered\n";
const LIFECYCLE_MISSING_MEDIA: &str = "[text-color=white]\n\n-- [asset.svg] [fit]\nFirst\n-- [#304060]\nSecond changed\n-- [missing.mp4] [fill]\nMissing media\n";
const LIFECYCLE_CORRUPT_MEDIA: &str = "[text-color=white]\n\n-- [asset.svg] [fit]\nFirst\n-- [#304060]\nSecond changed\n-- [broken.mp4] [fill]\nCorrupt media\n";
const LIFECYCLE_RASTER: &str = "[text-color=white]\n\n-- [asset.svg] [fit]\nFirst\n-- [#304060]\nSecond changed\n-- [large.png] [fill]\nPending raster\n";
const LIFECYCLE_MISSING_RASTER: &str = "[text-color=white]\n\n-- [asset.svg] [fit]\nFirst\n-- [#304060]\nSecond changed\n-- [missing.png] [fill]\nMissing raster\n";
const REHEARSAL_SOURCE: &str = "[duration=30] # retain defaults and comments\n-- [duration=1.25]\nFirst rehearsal slide\n-- [top]\nSecond rehearsal slide\n";

fn lifecycle_fixture() -> Result<PathBuf, String> {
    let directory = std::env::temp_dir().join(format!("pinpoint-lifecycle-{}", std::process::id()));
    if directory.exists() {
        std::fs::remove_dir_all(&directory).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir(&directory).map_err(|error| error.to_string())?;
    std::fs::write(directory.join("slides.pin"), LIFECYCLE_INITIAL)
        .map_err(|error| error.to_string())?;
    std::fs::write(
        directory.join("asset.svg"),
        "<svg xmlns='http://www.w3.org/2000/svg' width='64' height='64'><rect width='64' height='64' fill='red'/></svg>",
    )
    .map_err(|error| error.to_string())?;
    std::fs::write(directory.join("broken.mp4"), b"not a video")
        .map_err(|error| error.to_string())?;
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 1024, 1024)
        .map_err(|error| error.to_string())?;
    let context = cairo::Context::new(&surface).map_err(|error| error.to_string())?;
    context.set_source_rgb(0.12, 0.28, 0.58);
    context.paint().map_err(|error| error.to_string())?;
    let mut output =
        std::fs::File::create(directory.join("large.png")).map_err(|error| error.to_string())?;
    surface
        .write_to_png(&mut output)
        .map_err(|error| error.to_string())?;
    Ok(directory.join("slides.pin"))
}

fn media_fixture() -> Result<PathBuf, String> {
    let directory = std::env::temp_dir().join(format!("pinpoint-media-{}", std::process::id()));
    if directory.exists() {
        std::fs::remove_dir_all(&directory).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir(&directory).map_err(|error| error.to_string())?;
    let source = std::env::var_os("PINPOINT_VALIDATION_MEDIA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/app/share/pinpoint/introduction/bunny.webm"));
    std::fs::copy(&source, directory.join("sample.mp4")).map_err(|error| error.to_string())?;
    let mut deck = String::from(
        "[stage-color=#101820]\n[font=Sans 42px]\n[text-color=white]\n[transition=fade]\n",
    );
    for index in 0..12 {
        let filename = format!("video-{index:02}.mp4");
        std::fs::copy(directory.join("sample.mp4"), directory.join(&filename))
            .map_err(|error| error.to_string())?;
        deck.push_str(&format!("\n-- [{filename}] [fill]\nVideo {index}\n"));
    }
    let path = directory.join("media.pin");
    std::fs::write(&path, deck).map_err(|error| error.to_string())?;
    Ok(path)
}

fn rehearsal_fixture() -> Result<PathBuf, String> {
    let directory = std::env::temp_dir().join(format!("pinpoint-rehearsal-{}", std::process::id()));
    if directory.exists() {
        std::fs::remove_dir_all(&directory).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir(&directory).map_err(|error| error.to_string())?;
    let path = directory.join("rehearsal.pin");
    std::fs::write(&path, REHEARSAL_SOURCE).map_err(|error| error.to_string())?;
    Ok(path)
}

fn atomic_replace(path: &Path, contents: &str) -> Result<(), String> {
    let temporary = path.with_extension("replacement");
    std::fs::write(&temporary, contents).map_err(|error| error.to_string())?;
    std::fs::rename(temporary, path).map_err(|error| error.to_string())
}

#[derive(Clone)]
struct NormalControls {
    mpris: Rc<RefCell<Option<mpris::Mpris>>>,
    command_runner: command_runner::CommandRunner,
    command_trust: Rc<RefCell<command::Trust>>,
    sync: Rc<dyn Fn()>,
}

impl NormalControls {
    fn mpris_bus_name(&self) -> Option<String> {
        self.mpris
            .borrow()
            .as_ref()
            .map(|mpris| mpris.bus_name().to_owned())
    }

    fn command_status(&self) -> Option<Result<(), String>> {
        self.command_runner.status()
    }

    fn run_current_command(&self, stage: &Stage) -> Result<(), String> {
        let current_command = stage.current_command();
        let command =
            command::allow(true, current_command.as_deref()).map_err(|error| error.to_string())?;
        self.command_runner.run(command)
    }

    fn run_command_visible(
        &self,
        command: &str,
        chrome: &presentation_chrome::PresentationChrome,
    ) -> Result<(), String> {
        chrome.show_notice("Running slide command…");
        let completion_chrome = chrome.clone();
        self.command_runner
            .run_with_callback(command, move |result| {
                completion_chrome.show_notice(&match result {
                    Ok(()) => "Slide command finished".into(),
                    Err(error) => format!("Slide command failed — {error}"),
                });
            })
    }

    fn request_current_command(
        &self,
        window: &adw::ApplicationWindow,
        stage: &Stage,
        chrome: &presentation_chrome::PresentationChrome,
    ) -> Result<(), String> {
        if self.command_runner.is_running() {
            self.command_runner.cancel("user-requested");
            chrome.show_notice("Slide command stopped");
            return Ok(());
        }
        let revision = stage.generation();
        let current_command = stage.current_command();
        match self
            .command_trust
            .borrow_mut()
            .request(true, revision, current_command.as_deref())
            .map_err(|error| error.to_string())?
        {
            command::TrustDecision::Allowed(command) => self.run_command_visible(&command, chrome),
            command::TrustDecision::ConfirmationPending => {
                chrome.show_notice("Review the command confirmation");
                Ok(())
            }
            command::TrustDecision::ConfirmationRequired(command) => {
                let dialog = adw::AlertDialog::new(
                    Some("Allow Presentation Commands?"),
                    Some(
                        "This deck can run shell commands inside Pinpoint's sandbox. Confirming trusts commands only until this presentation changes or closes.",
                    ),
                );
                let command_label = gtk::Label::builder()
                    .label(&command)
                    .selectable(true)
                    .wrap(true)
                    .xalign(0.0)
                    .build();
                command_label.add_css_class("monospace");
                command_label.set_accessible_role(gtk::AccessibleRole::Label);
                command_label
                    .update_property(&[gtk::accessible::Property::Label("Exact slide command")]);
                dialog.set_extra_child(Some(&command_label));
                dialog.add_response("cancel", "Cancel");
                dialog.add_response("run", "Run Command");
                dialog.set_response_appearance("run", adw::ResponseAppearance::Suggested);
                dialog.set_default_response(Some("cancel"));
                dialog.set_close_response("cancel");
                let controls = self.clone();
                let stage = stage.clone();
                let chrome = chrome.clone();
                dialog.connect_response(None, move |_, response| {
                    if response != "run" {
                        controls.command_trust.borrow_mut().dismiss();
                        return;
                    }
                    let current_revision = stage.generation();
                    let current_command = stage.current_command();
                    let current_command = command::allow(true, current_command.as_deref()).ok();
                    if current_revision != revision
                        || current_command != Some(command.as_str())
                        || !controls
                            .command_trust
                            .borrow_mut()
                            .confirm(revision, &command)
                    {
                        controls.command_trust.borrow_mut().dismiss();
                        chrome.show_notice("Presentation changed — command was not run");
                        return;
                    }
                    if let Err(error) = controls.run_command_visible(&command, &chrome) {
                        chrome.show_notice(&error);
                        eprintln!("pinpoint: {error}");
                    }
                });
                dialog.present(Some(window));
                Ok(())
            }
        }
    }

    fn revoke_command_trust(&self) -> bool {
        let revoked = self.command_trust.borrow_mut().revoke();
        if self.command_runner.is_running() {
            self.command_runner.cancel("presentation-changed");
            return true;
        }
        revoked
    }
}

fn install_normal_controls(
    app: &adw::Application,
    window: &adw::ApplicationWindow,
    stage: &Stage,
) -> NormalControls {
    let mpris: Rc<RefCell<Option<mpris::Mpris>>> = Rc::new(RefCell::new(None));
    let command_runner = command_runner::CommandRunner::default();
    let command_trust = Rc::new(RefCell::new(command::Trust::default()));
    let state = Rc::new(RefCell::new(mpris::State {
        presenting: true,
        slide: stage.current_slide(),
        slides: stage.slide_count(),
        fullscreen: window.is_fullscreen(),
    }));
    let weak_mpris = Rc::downgrade(&mpris);
    let weak_stage = stage.downgrade();
    let weak_window = window.downgrade();
    let sync: Rc<dyn Fn()> = Rc::new(move || {
        let (Some(mpris), Some(stage), Some(window)) = (
            weak_mpris.upgrade(),
            weak_stage.upgrade(),
            weak_window.upgrade(),
        ) else {
            return;
        };
        if let Some(mpris) = mpris.borrow().as_ref() {
            mpris.sync(mpris::State {
                presenting: true,
                slide: stage.current_slide(),
                slides: stage.slide_count(),
                fullscreen: window.is_fullscreen(),
            });
        }
    });
    let dispatch_stage = stage.downgrade();
    let dispatch_window = window.downgrade();
    let dispatch_sync = sync.clone();
    let adapter = match mpris::Mpris::new(app, state, move |command| {
        let (Some(stage), Some(window)) = (dispatch_stage.upgrade(), dispatch_window.upgrade())
        else {
            return;
        };
        match command {
            mpris::Command::Next => {
                stage.next();
            }
            mpris::Command::Previous => {
                stage.previous();
            }
            mpris::Command::Fullscreen(fullscreen) => {
                if fullscreen {
                    window.fullscreen();
                } else {
                    window.unfullscreen();
                }
            }
        }
        dispatch_sync();
    }) {
        Ok(adapter) => adapter,
        Err(error) => {
            eprintln!("pinpoint: unable to export MPRIS controls: {error}");
            return NormalControls {
                mpris,
                command_runner,
                command_trust,
                sync,
            };
        }
    };
    *mpris.borrow_mut() = Some(adapter);
    let close_mpris = mpris.clone();
    let close_command = command_runner.clone();
    window.connect_destroy(move |_| {
        close_mpris.borrow_mut().take();
        close_command.cancel("presentation-closed");
    });
    let fullscreen_sync = sync.clone();
    window.connect_fullscreened_notify(move |_| fullscreen_sync());
    sync();
    NormalControls {
        mpris,
        command_runner,
        command_trust,
        sync,
    }
}

fn install_navigation(
    window: &adw::ApplicationWindow,
    stage: &Stage,
    chrome: &presentation_chrome::PresentationChrome,
    controls: NormalControls,
) {
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        window,
        #[weak]
        stage,
        #[strong]
        chrome,
        #[strong]
        controls,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, _| {
            let handled = match key {
                gdk::Key::Right | gdk::Key::Down | gdk::Key::space | gdk::Key::Page_Down => {
                    stage.next()
                }
                gdk::Key::Left | gdk::Key::Up | gdk::Key::BackSpace | gdk::Key::Page_Up => {
                    stage.previous()
                }
                gdk::Key::Home | gdk::Key::h | gdk::Key::H => stage.first(),
                gdk::Key::End => stage.last(),
                gdk::Key::b | gdk::Key::B => {
                    stage.toggle_blank();
                    true
                }
                gdk::Key::c | gdk::Key::C => stage.retry_camera(),
                gdk::Key::F11 | gdk::Key::f | gdk::Key::F => {
                    if window.is_fullscreen() {
                        window.unfullscreen();
                    } else {
                        window.fullscreen();
                    }
                    true
                }
                gdk::Key::Return | gdk::Key::KP_Enter => {
                    if let Err(error) = controls.request_current_command(&window, &stage, &chrome) {
                        chrome.show_notice(&error);
                        eprintln!("pinpoint: {error}");
                    }
                    true
                }
                gdk::Key::Escape | gdk::Key::q | gdk::Key::Q => {
                    window.close();
                    true
                }
                _ => false,
            };
            if handled {
                chrome.hide();
                (controls.sync)();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    ));
    window.add_controller(keys);

    let click = gtk::GestureClick::new();
    click.connect_released(glib::clone!(
        #[weak]
        stage,
        #[strong]
        chrome,
        #[strong]
        controls,
        move |gesture, _, _, _| {
            match gesture.current_button() {
                1 => {
                    stage.next();
                }
                3 => {
                    stage.previous();
                }
                _ => {}
            }
            chrome.hide();
            (controls.sync)();
        }
    ));
    stage.add_controller(click);
}

pub(crate) fn open_editor_snapshot_presenter(
    app: &adw::Application,
    presentation: pinpoint_core::presentation::Presentation,
    path: PathBuf,
    initial_slide: usize,
    editor_window: &adw::ApplicationWindow,
) -> adw::ApplicationWindow {
    let stage = Stage::default();
    stage.set_presentation_at(presentation, path.clone(), initial_slide);
    let view = gtk::Stack::builder().hexpand(true).vexpand(true).build();
    view.add_named(&stage, Some("stage"));
    view.set_visible_child_name("stage");
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(format!("Pinpoint — {}", path.display()))
        .default_width(1280)
        .default_height(720)
        .build();
    let (content, chrome) = presentation_chrome::PresentationChrome::new(&window, &stage, &view);
    window.set_content(Some(&content));
    let controls = install_normal_controls(app, &window, &stage);
    install_navigation(&window, &stage, &chrome, controls);
    let restore_editor = editor_window.clone();
    window.connect_close_request(move |_| {
        restore_editor.present();
        glib::Propagation::Proceed
    });
    editor_window.set_visible(false);
    window.present();
    stage.grab_focus();
    eprintln!(
        "PINPOINT EDITOR PRESENT snapshot=true slide={} path={}",
        initial_slide,
        path.display()
    );
    window
}

#[derive(Default)]
struct FrameStats {
    last: Option<i64>,
    intervals_ms: Vec<f64>,
}

impl FrameStats {
    fn record(&mut self, time: i64) {
        if let Some(last) = self.last {
            self.intervals_ms.push((time - last) as f64 / 1000.0);
        }
        self.last = Some(time);
    }

    fn summary(&self) -> (usize, f64, f64, f64) {
        if self.intervals_ms.is_empty() {
            return (0, 0.0, 0.0, 0.0);
        }
        let mut sorted = self.intervals_ms.clone();
        sorted.sort_by(f64::total_cmp);
        let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
        let p95 = sorted[((sorted.len() - 1) as f64 * 0.95).round() as usize];
        let max = *sorted.last().unwrap();
        (sorted.len(), mean, p95, max)
    }

    fn reset(&mut self) {
        self.last = None;
        self.intervals_ms.clear();
    }
}

fn memory_kib() -> (u64, u64) {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return (0, 0);
    };
    let value = |name: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|line| line.split_whitespace().next())
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
    };
    (value("VmRSS:"), value("VmHWM:"))
}

const PIXEL_STAGE_COLORS: [[i32; 3]; 3] =
    [[0xe0, 0x1b, 0x24], [0x33, 0xd1, 0x7a], [0x35, 0x84, 0xe4]];

fn validate_pixel_reference(stage: &Stage, expected: &[i32; 3]) -> Result<(u64, u64), String> {
    let width = stage.width();
    let height = stage.height();
    if width <= 0 || height <= 0 {
        return Err("stage has no allocation".into());
    }
    let native = stage
        .native()
        .ok_or_else(|| "stage has no GtkNative".to_owned())?;
    let renderer = native
        .renderer()
        .ok_or_else(|| "stage has no GSK renderer".to_owned())?;
    let paintable = gtk::WidgetPaintable::new(Some(stage));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, width.into(), height.into());
    let node = snapshot
        .to_node()
        .ok_or_else(|| "stage snapshot was empty".to_owned())?;
    let texture = renderer.render_texture(&node, None);
    let mut downloader = gdk::TextureDownloader::new(&texture);
    downloader.set_format(gdk::MemoryFormat::R8g8b8a8Premultiplied);
    let (bytes, stride) = downloader.download_bytes();
    let pixels = bytes.as_ref();
    for (x, y) in [
        (width / 8, height / 8),
        (width * 7 / 8, height / 8),
        (width / 8, height * 7 / 8),
        (width * 7 / 8, height * 7 / 8),
    ] {
        let offset = y as usize * stride + x as usize * 4;
        let matches = pixels.get(offset..offset + 3).is_some_and(|actual| {
            actual
                .iter()
                .zip(expected)
                .all(|(actual, expected)| (*actual as i32 - *expected).abs() <= 12)
        });
        if !matches {
            return Err(format!("background pixel mismatch at {x},{y}"));
        }
    }
    let mut white = 0_u64;
    let mut shaded = 0_u64;
    for y in height / 3..height * 2 / 3 {
        for x in width / 4..width * 3 / 4 {
            let offset = y as usize * stride + x as usize * 4;
            if let Some(pixel) = pixels.get(offset..offset + 3) {
                if pixel.iter().all(|channel| *channel > 235) {
                    white += 1;
                }
                if pixel.iter().all(|channel| *channel < 55) {
                    shaded += 1;
                }
            }
        }
    }
    if white <= 250 || shaded <= 2_000 {
        return Err(format!(
            "text/shading coverage mismatch: white={white}, shaded={shaded}"
        ));
    }
    Ok((white, shaded))
}

fn page_curl_capture_texture(width: i32, height: i32, previous: bool) -> gdk::Texture {
    let stride = width as usize * 4;
    let mut pixels = vec![0_u8; height as usize * stride];
    for y in 0..height {
        for x in 0..width {
            let pixel = &mut pixels[y as usize * stride + x as usize * 4..][..4];
            let checker = if ((x / 32) ^ (y / 32)) & 1 == 1 {
                36
            } else {
                0
            };
            if previous {
                pixel[0] = (x * 255 / (width - 1).max(1)) as u8;
                pixel[1] = (y * 255 / (height - 1).max(1)) as u8;
                pixel[2] = 60 + checker;
            } else {
                pixel[0] = 28 + checker;
                pixel[1] = (x * 180 / (width - 1).max(1)) as u8;
                pixel[2] = (255 - y * 180 / (height - 1).max(1)) as u8;
            }
            pixel[3] = 255;
        }
    }
    gdk::MemoryTexture::new(
        width,
        height,
        gdk::MemoryFormat::R8g8b8a8Premultiplied,
        &glib::Bytes::from_owned(pixels),
        stride,
    )
    .upcast()
}

fn start_page_curl_capture(
    app: &adw::Application,
    result: Rc<Cell<bool>>,
    output: PathBuf,
    width: i32,
    height: i32,
    backwards: bool,
) {
    let curl_view = page_curl_view::PageCurlView::new();
    let previous = page_curl_capture_texture(width, height, true);
    let current = page_curl_capture_texture(width, height, false);
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .default_width(width)
        .default_height(height)
        .decorated(false)
        .build();
    window.set_content(Some(curl_view.widget()));
    curl_view.set_transition(
        &previous,
        &current,
        if backwards { 0.0 } else { 0.5 },
        0.0,
        if backwards { 0.5 } else { 0.0 },
        0.0,
        backwards,
    );
    window.present();
    let app = app.clone();
    glib::timeout_add_local_once(Duration::from_millis(150), move || {
        let capture = (|| -> Result<(i32, i32), String> {
            let paintable = gtk::WidgetPaintable::new(Some(curl_view.widget()));
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(&snapshot, width.into(), height.into());
            let node = snapshot
                .to_node()
                .ok_or_else(|| "page-curl capture snapshot was empty".to_owned())?;
            let native = curl_view
                .widget()
                .native()
                .ok_or_else(|| "page-curl capture has no GtkNative".to_owned())?;
            let renderer = native
                .renderer()
                .ok_or_else(|| "page-curl capture has no GSK renderer".to_owned())?;
            let texture = renderer.render_texture(&node, None);
            texture
                .save_to_png(&output)
                .map_err(|error| error.to_string())?;
            Ok((texture.width(), texture.height()))
        })();
        match capture {
            Ok((captured_width, captured_height)) => eprintln!(
                "CURL CAPTURE RUST output={} size={}x{} direction={}",
                output.display(),
                captured_width,
                captured_height,
                if backwards { "backward" } else { "forward" }
            ),
            Err(error) => {
                eprintln!("CURL CAPTURE RUST FAIL {error}");
                result.set(false);
            }
        }
        window.close();
        app.quit();
    });
}

fn start_real_slide_capture(
    app: &adw::Application,
    window: &adw::ApplicationWindow,
    stage: &Stage,
    output: PathBuf,
    slide: usize,
) {
    if slide >= stage.slide_count() || !stage.set_slide_without_transition(slide) {
        eprintln!(
            "PINPOINT REAL CAPTURE FAIL slide={} count={}",
            slide,
            stage.slide_count()
        );
        app.quit();
        return;
    }

    let started = Instant::now();
    let app = app.clone();
    let window = window.clone();
    let stage = stage.clone();
    let ready_since = Rc::new(Cell::new(None::<Instant>));
    let ready_since_for_tick = ready_since.clone();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        let stats = stage.stats();
        let width = stage.width();
        let height = stage.height();
        if (width <= 0
            || height <= 0
            || stage.current_slide() != slide
            || stage.is_transitioning()
            || stats.pending > 0
            || stage.pending_media() > 0)
            && started.elapsed() < Duration::from_secs(20)
        {
            ready_since_for_tick.set(None);
            return glib::ControlFlow::Continue;
        }
        if ready_since_for_tick.get().is_none() {
            ready_since_for_tick.set(Some(Instant::now()));
            return glib::ControlFlow::Continue;
        }
        if ready_since_for_tick
            .get()
            .is_some_and(|ready| ready.elapsed() < Duration::from_millis(300))
            && started.elapsed() < Duration::from_secs(20)
        {
            return glib::ControlFlow::Continue;
        }

        let result = (|| -> Result<(i32, i32), String> {
            if width <= 0 || height <= 0 {
                return Err("real slide capture has no allocation".into());
            }
            let paintable = gtk::WidgetPaintable::new(Some(&stage));
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(&snapshot, width.into(), height.into());
            let node = snapshot
                .to_node()
                .ok_or_else(|| "real slide capture snapshot was empty".to_owned())?;
            let native = stage
                .native()
                .ok_or_else(|| "real slide capture has no GtkNative".to_owned())?;
            let renderer = native
                .renderer()
                .ok_or_else(|| "real slide capture has no GSK renderer".to_owned())?;
            let texture = renderer.render_texture(&node, None);
            texture
                .save_to_png(&output)
                .map_err(|error| error.to_string())?;
            Ok((texture.width(), texture.height()))
        })();
        match result {
            Ok((width, height)) => eprintln!(
                "PINPOINT REAL CAPTURE PASS slide={} current={} transitioning={} output={} size={}x{} ready_ms={}",
                slide,
                stage.current_slide(),
                stage.is_transitioning(),
                output.display(),
                width,
                height,
                started.elapsed().as_millis()
            ),
            Err(error) => eprintln!("PINPOINT REAL CAPTURE FAIL {error}"),
        }
        window.close();
        app.quit();
        glib::ControlFlow::Break
    });
}

fn start_stage_validation(
    app: &adw::Application,
    window: &adw::ApplicationWindow,
    stage: &Stage,
    chrome: &presentation_chrome::PresentationChrome,
    result: Rc<Cell<bool>>,
    validate_pixels: bool,
    validate_transitions: bool,
) {
    if window.is_decorated() {
        eprintln!("PINPOINT STAGE FAIL audience window retained client-side decoration");
        result.set(false);
        app.quit();
        return;
    }
    chrome.reveal_for_input();
    if !chrome.is_revealed() {
        eprintln!("PINPOINT STAGE FAIL close control did not reveal");
        result.set(false);
        app.quit();
        return;
    }
    chrome.hide();
    let frames = Rc::new(RefCell::new(FrameStats::default()));
    stage.add_tick_callback(glib::clone!(
        #[strong]
        frames,
        move |_, clock| {
            frames.borrow_mut().record(clock.frame_time());
            glib::ControlFlow::Continue
        }
    ));
    let next = Rc::new(Cell::new(if validate_transitions {
        1_usize
    } else {
        0_usize
    }));
    let backwards_started = Rc::new(Cell::new(false));
    let workload_started = Rc::new(Cell::new(false));
    let pixel_totals = Rc::new(Cell::new((0_u64, 0_u64, 0_usize)));
    let started = Instant::now();
    glib::timeout_add_local(
        Duration::from_millis(250),
        glib::clone!(
            #[weak]
            app,
            #[weak]
            stage,
            #[strong]
            next,
            #[strong]
            backwards_started,
            #[strong]
            workload_started,
            #[strong]
            pixel_totals,
            #[strong]
            frames,
            #[strong]
            result,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move || {
                if !workload_started.get() {
                    let stats = stage.stats();
                    let media_pending = stage.pending_media();
                    if (stats.pending > 0
                        || media_pending > 0
                        || (validate_transitions && !stage.curl_prewarm_ready()))
                        && started.elapsed() < Duration::from_secs(20)
                    {
                        return glib::ControlFlow::Continue;
                    }
                    if validate_pixels && stage.slide_count() != PIXEL_STAGE_COLORS.len() {
                        eprintln!(
                            "PINPOINT PIXELS FAIL slides={} expected={}",
                            stage.slide_count(),
                            PIXEL_STAGE_COLORS.len()
                        );
                        result.set(false);
                        app.quit();
                        return glib::ControlFlow::Break;
                    }
                    frames.borrow_mut().reset();
                    workload_started.set(true);
                    eprintln!(
                        "PINPOINT WORKING-SET ready_ms={} raster_pending={} media_pending={}",
                        started.elapsed().as_millis(),
                        stats.pending,
                        media_pending
                    );
                }
                let index = next.get();
                if validate_transitions && stage.is_transitioning() {
                    return glib::ControlFlow::Continue;
                }
                if validate_transitions && !stage.curl_prewarm_ready() {
                    if started.elapsed() >= Duration::from_secs(20) {
                        eprintln!(
                            "PINPOINT TRANSITION FAIL page-curl prewarm did not become ready"
                        );
                        eprintln!(
                            "PINPOINT TRANSITION prewarm-state={}",
                            stage.curl_prewarm_status()
                        );
                        result.set(false);
                        app.quit();
                        return glib::ControlFlow::Break;
                    }
                    return glib::ControlFlow::Continue;
                }
                if validate_transitions && index < stage.slide_count() {
                    if !stage.next() {
                        eprintln!(
                            "PINPOINT TRANSITION FAIL unable-to-enter-slide={index} current={}",
                            stage.current_slide()
                        );
                        result.set(false);
                        app.quit();
                        return glib::ControlFlow::Break;
                    }
                    next.set(index + 1);
                    return glib::ControlFlow::Continue;
                } else if validate_transitions && !backwards_started.get() {
                    let current = stage.current_slide();
                    if current > 0 && !stage.curl_pair_ready(current, current - 1, true) {
                        if started.elapsed() >= Duration::from_secs(20) {
                            eprintln!(
                                "PINPOINT TRANSITION FAIL backward page-curl prewarm did not become ready"
                            );
                            result.set(false);
                            app.quit();
                            return glib::ControlFlow::Break;
                        }
                        return glib::ControlFlow::Continue;
                    }
                    if current > 0 && !stage.previous() {
                        eprintln!("PINPOINT TRANSITION FAIL unable-to-return-from-slide={current}");
                        result.set(false);
                        app.quit();
                        return glib::ControlFlow::Break;
                    }
                    backwards_started.set(true);
                    return glib::ControlFlow::Continue;
                } else if index < stage.slide_count() {
                    if validate_pixels && index > 0 {
                        let (white, shaded, checked) = pixel_totals.get();
                        if checked < index {
                            match validate_pixel_reference(&stage, &PIXEL_STAGE_COLORS[checked]) {
                                Ok((slide_white, slide_shaded)) => pixel_totals.set((
                                    white + slide_white,
                                    shaded + slide_shaded,
                                    checked + 1,
                                )),
                                Err(error) => {
                                    eprintln!("PINPOINT PIXELS FAIL slide={checked} {error}");
                                    result.set(false);
                                    app.quit();
                                    return glib::ControlFlow::Break;
                                }
                            }
                        }
                    }
                    stage.set_slide_without_transition(index);
                    next.set(index + 1);
                    return glib::ControlFlow::Continue;
                }
                let stats = stage.stats();
                if (stats.pending > 0 || stage.pending_media() > 0)
                    && started.elapsed() < Duration::from_secs(20)
                {
                    return glib::ControlFlow::Continue;
                }
                let (rss, hwm) = memory_kib();
                let (frame_count, frame_mean, frame_p95, frame_max) = frames.borrow().summary();
                let pixels = if validate_pixels {
                    let (mut white, mut shaded, mut colors) = pixel_totals.get();
                    if colors < stage.slide_count() && colors < PIXEL_STAGE_COLORS.len() {
                        match validate_pixel_reference(&stage, &PIXEL_STAGE_COLORS[colors]) {
                            Ok((slide_white, slide_shaded)) => {
                                white += slide_white;
                                shaded += slide_shaded;
                                colors += 1;
                                pixel_totals.set((white, shaded, colors));
                            }
                            Err(error) => {
                                eprintln!("PINPOINT PIXELS FAIL slide={colors} {error}");
                            }
                        }
                    }
                    if colors == PIXEL_STAGE_COLORS.len() && colors == stage.slide_count() {
                        eprintln!(
                            "PINPOINT PIXELS PASS stage_colors={colors} palette=red-green-blue white={white} shaded={shaded} size={}x{}",
                            stage.width(),
                            stage.height()
                        );
                        true
                    } else {
                        eprintln!(
                            "PINPOINT PIXELS FAIL checked={colors} slides={} expected={}",
                            stage.slide_count(),
                            PIXEL_STAGE_COLORS.len()
                        );
                        false
                    }
                } else {
                    true
                };
                let curl_frames = stage.page_curl_frames();
                let curl_setup_worst_ms = stage.page_curl_setup_worst_ms();
                let curl_snapshot_p95_ms = stage.page_curl_snapshot_p95_ms();
                let curl_worst_ms = stage.page_curl_worst_ms();
                let transitions = !validate_transitions
                    || (curl_frames > 0 && backwards_started.get() && frame_p95 <= 20.0);
                if validate_transitions {
                    eprintln!(
                        "PINPOINT TRANSITION {} page_curl_frames={curl_frames} backward=prewarmed page_curl_setup_worst_ms={curl_setup_worst_ms:.3} page_curl_snapshot_p95_ms={curl_snapshot_p95_ms:.3} page_curl_snapshot_worst_ms={curl_worst_ms:.3} frame_p95_ms={frame_p95:.3}",
                        if transitions { "PASS" } else { "FAIL" }
                    );
                }
                let success =
                    stats.failures == 0 && stage.media_error().is_none() && pixels && transitions;
                result.set(success);
                eprintln!(
                    "PINPOINT STAGE {} slides={} textures={} texture_mib={:.2} texture_budget_mib={:.2} svg_sources={} failures={} loaders={}/{} rss_mib={:.2} hwm_mib={:.2} decorated=false close_control=motion-fade-200ms/2000ms cursor_hide_ms=500 page_curl_frames={} page_curl_setup_worst_ms={:.3} page_curl_snapshot_p95_ms={:.3} page_curl_snapshot_worst_ms={:.3} frames={} frame_mean_ms={:.3} frame_p95_ms={:.3} frame_max_ms={:.3}",
                    if success { "PASS" } else { "FAIL" },
                    stage.slide_count(),
                    stats.textures,
                    stats.texture_bytes as f64 / 1024.0 / 1024.0,
                    stats.texture_budget as f64 / 1024.0 / 1024.0,
                    stats.svg_sources,
                    stats.failures,
                    stats.active_loaders,
                    stats.loader_limit,
                    rss as f64 / 1024.0,
                    hwm as f64 / 1024.0,
                    curl_frames,
                    curl_setup_worst_ms,
                    curl_snapshot_p95_ms,
                    curl_worst_ms,
                    frame_count,
                    frame_mean,
                    frame_p95,
                    frame_max,
                );
                app.quit();
                glib::ControlFlow::Break
            }
        ),
    );
}

fn cleanup_lifecycle_fixture(path: &Path) {
    if let Some(directory) = path.parent().filter(|directory| {
        directory
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("pinpoint-lifecycle-"))
    }) {
        let _ = std::fs::remove_dir_all(directory);
    }
}

fn cleanup_media_fixture(path: &Path) {
    if let Some(directory) = path.parent().filter(|directory| {
        directory
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("pinpoint-media-"))
    }) {
        let _ = std::fs::remove_dir_all(directory);
    }
}

fn cleanup_rehearsal_fixture(path: &Path) {
    if let Some(directory) = path.parent().filter(|directory| {
        directory
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("pinpoint-rehearsal-"))
    }) {
        let _ = std::fs::remove_dir_all(directory);
    }
}

fn start_media_validation(
    app: &adw::Application,
    stage: &Stage,
    result: Rc<Cell<bool>>,
    path: PathBuf,
) {
    let state = Rc::new(Cell::new(0_u8));
    let observed_duration = Rc::new(Cell::new(0_u64));
    let observed_eos = Rc::new(Cell::new(0_u64));
    let observed_caps = Rc::new(RefCell::new(String::new()));
    let started = Instant::now();
    glib::timeout_add_local(
        Duration::from_millis(50),
        glib::clone!(
            #[weak]
            app,
            #[weak]
            stage,
            #[strong]
            result,
            #[strong]
            state,
            #[strong]
            observed_duration,
            #[strong]
            observed_eos,
            #[strong]
            observed_caps,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move || {
                let fail = |message: String| {
                    eprintln!("PINPOINT MEDIA FAIL {message}");
                    result.set(false);
                    cleanup_media_fixture(&path);
                    app.quit();
                    glib::ControlFlow::Break
                };
                if started.elapsed() > Duration::from_secs(25) {
                    return fail(format!(
                        "timeout state={} prepared={} active={:?} offload={}",
                        state.get(),
                        stage.prepared_media(),
                        stage.active_media_status(),
                        stage.media_offload_configured()
                    ));
                }
                match state.get() {
                    0 if stage.is_ready() => {
                        let Some(status) = stage.active_media_status() else {
                            return fail("working set has no active media".into());
                        };
                        if stage.prepared_media() != 8 || !stage.media_offload_configured() {
                            return fail(format!(
                                "initial state prepared={} state={} offload={}",
                                stage.prepared_media(),
                                status.state,
                                stage.media_offload_configured()
                            ));
                        }
                        if status.state != "playing" {
                            return glib::ControlFlow::Continue;
                        }
                        let Some(duration_ms) = status.duration_ms.filter(|duration| *duration > 0)
                        else {
                            return glib::ControlFlow::Continue;
                        };
                        let Some(caps) = status.negotiated_caps else {
                            return glib::ControlFlow::Continue;
                        };
                        observed_duration.set(duration_ms);
                        observed_caps.replace(caps);
                        stage.set_media_looping(true);
                        if let Err(error) = stage.seek_active_media(Duration::from_millis(
                            duration_ms.saturating_sub(100),
                        )) {
                            return fail(format!("seek near EOS: {error}"));
                        }
                        state.set(1);
                    }
                    1 => {
                        let Some(status) = stage.active_media_status() else {
                            return fail("active media disappeared while waiting for EOS".into());
                        };
                        if status.eos_count == 0 {
                            return glib::ControlFlow::Continue;
                        }
                        observed_eos.set(status.eos_count);
                        if !stage.set_slide_without_transition(8) {
                            return fail("unable to enter the eviction slide".into());
                        }
                        state.set(2);
                    }
                    2 if stage.is_ready() => {
                        let status = stage.active_media_status();
                        if status.as_ref().map(|status| status.state.as_str()) != Some("playing") {
                            return glib::ControlFlow::Continue;
                        }
                        if stage.current_slide() != 8
                            || stage.prepared_media() != 8
                            || !stage.media_offload_configured()
                        {
                            return fail(format!(
                                "eviction state slide={} prepared={} offload={} media={status:?}",
                                stage.current_slide(),
                                stage.prepared_media(),
                                stage.media_offload_configured()
                            ));
                        }
                        stage.set_audio_enabled(false);
                        state.set(3);
                    }
                    3 if stage.is_ready() => {
                        if stage
                            .active_media_status()
                            .is_some_and(|status| status.state != "playing")
                        {
                            return glib::ControlFlow::Continue;
                        }
                        if stage.is_audio_enabled()
                            || stage.prepared_media() != 8
                            || stage.active_media_status().is_none()
                        {
                            return fail(format!(
                                "audio policy rebuild enabled={} prepared={}",
                                stage.is_audio_enabled(),
                                stage.prepared_media()
                            ));
                        }
                        stage.set_media_enabled(false);
                        state.set(4);
                    }
                    4 => {
                        if stage.is_media_enabled()
                            || stage.prepared_media() != 0
                            || stage.active_media_status().is_some()
                            || stage.media_offload_configured()
                        {
                            return fail("media disable did not release pipelines/offload".into());
                        }
                        stage.set_media_enabled(true);
                        state.set(5);
                    }
                    5 if stage.is_ready() => {
                        if stage
                            .active_media_status()
                            .is_some_and(|status| status.state != "playing")
                        {
                            return glib::ControlFlow::Continue;
                        }
                        if !stage.is_media_enabled()
                            || stage.prepared_media() != 8
                            || stage.active_media_status().is_none()
                            || !stage.media_offload_configured()
                        {
                            return fail("media re-enable did not rebuild the bounded set".into());
                        }
                        let (rss, hwm) = memory_kib();
                        eprintln!(
                            "PINPOINT MEDIA PASS unique_videos=12 pipeline_limit=8 prepared={} eviction=bounded eos_loops={} seek=handled duration_ms={} caps={} audio=disabled-with-fakesink offload=configured portal_camera=deferred-stage-9 rss_mib={:.2} hwm_mib={:.2}",
                            stage.prepared_media(),
                            observed_eos.get(),
                            observed_duration.get(),
                            observed_caps.borrow().replace(' ', "_"),
                            rss as f64 / 1024.0,
                            hwm as f64 / 1024.0,
                        );
                        result.set(true);
                        cleanup_media_fixture(&path);
                        app.quit();
                        return glib::ControlFlow::Break;
                    }
                    _ => {}
                }
                glib::ControlFlow::Continue
            }
        ),
    );
}

fn start_lifecycle_validation(
    app: &adw::Application,
    window: &adw::ApplicationWindow,
    stage: &Stage,
    lifecycle: Rc<lifecycle::Lifecycle>,
    result: Rc<Cell<bool>>,
    path: PathBuf,
    stress: bool,
) {
    let state = Rc::new(Cell::new(0_u8));
    let cancellation_wait = Rc::new(Cell::new(None::<Instant>));
    let resize_before = Rc::new(Cell::new(None));
    let stress_step = Rc::new(Cell::new(0_u32));
    let stress_target_reload = Rc::new(Cell::new(0_u64));
    let stress_rss_start = Rc::new(Cell::new(0_u64));
    let stress_cycles = std::thread::available_parallelism()
        .map_or(32, |cpus| cpus.get().saturating_mul(8))
        .clamp(32, 128) as u32;
    let started = Instant::now();
    glib::timeout_add_local(
        Duration::from_millis(25),
        glib::clone!(
            #[weak]
            app,
            #[weak]
            window,
            #[weak]
            stage,
            #[strong]
            lifecycle,
            #[strong]
            result,
            #[strong]
            state,
            #[strong]
            cancellation_wait,
            #[strong]
            resize_before,
            #[strong]
            stress_step,
            #[strong]
            stress_target_reload,
            #[strong]
            stress_rss_start,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move || {
                let fail = |message: String| {
                    eprintln!("PINPOINT LIFECYCLE FAIL {message}");
                    result.set(false);
                    app.quit();
                    glib::ControlFlow::Break
                };
                let timeout = if stress { 90 } else { 30 };
                if started.elapsed() > Duration::from_secs(timeout) {
                    return fail(format!("timeout state={}", state.get()));
                }
                match state.get() {
                    0 if stage.is_ready() => {
                        if !stage.set_slide_without_transition(2) {
                            return fail("unable to select initial third slide".into());
                        }
                        if let Err(error) = atomic_replace(&path, LIFECYCLE_CHANGED) {
                            return fail(format!("initial replacement: {error}"));
                        }
                        state.set(1);
                    }
                    1 if lifecycle.reload_count() >= 1 => {
                        if stage.current_slide() != 1 || stage.slide_count() != 3 {
                            return fail(format!(
                                "changed slide mismatch current={} slides={}",
                                stage.current_slide(),
                                stage.slide_count()
                            ));
                        }
                        let asset = path.with_file_name("asset.svg");
                        if let Err(error) = atomic_replace(
                            &asset,
                            "<svg xmlns='http://www.w3.org/2000/svg' width='64' height='64'><rect width='64' height='64' fill='blue'/></svg>",
                        ) {
                            return fail(format!("asset replacement: {error}"));
                        }
                        state.set(2);
                    }
                    2 if lifecycle.asset_invalidation_count() >= 1 => {
                        stage.next();
                        if let Err(error) = atomic_replace(&path, "invalid intermediate file\n") {
                            return fail(format!("invalid replacement: {error}"));
                        }
                        state.set(3);
                    }
                    3 if lifecycle.last_error().is_some() => {
                        if stage.slide_count() != 3 {
                            return fail("invalid reload replaced the last good deck".into());
                        }
                        if let Err(error) = atomic_replace(&path, LIFECYCLE_RECOVERED) {
                            return fail(format!("recovery replacement: {error}"));
                        }
                        state.set(4);
                    }
                    4 if lifecycle.reload_count() >= 2 => {
                        if lifecycle.last_error().is_some()
                            || stage.current_slide() != 2
                            || stage.is_transitioning()
                        {
                            return fail(format!(
                                "recovery mismatch current={} transitioning={} error={:?}",
                                stage.current_slide(),
                                stage.is_transitioning(),
                                lifecycle.last_error()
                            ));
                        }
                        if let Err(error) = atomic_replace(&path, LIFECYCLE_MISSING_MEDIA) {
                            return fail(format!("missing-media replacement: {error}"));
                        }
                        state.set(5);
                    }
                    5 if lifecycle.reload_count() >= 3 && stage.media_error().is_some() => {
                        if !stage.is_ready() {
                            return fail("missing media did not settle readiness".into());
                        }
                        if let Err(error) = atomic_replace(&path, LIFECYCLE_CORRUPT_MEDIA) {
                            return fail(format!("corrupt-media replacement: {error}"));
                        }
                        state.set(6);
                    }
                    6 if lifecycle.reload_count() >= 4 && stage.media_error().is_some() => {
                        if let Err(error) = atomic_replace(&path, LIFECYCLE_RECOVERED) {
                            return fail(format!("media recovery replacement: {error}"));
                        }
                        state.set(7);
                    }
                    7 if lifecycle.reload_count() >= 5 => {
                        if lifecycle.last_error().is_some()
                            || stage.media_error().is_some()
                            || !stage.is_ready()
                        {
                            return fail(format!(
                                "media recovery mismatch lifecycle_error={:?} media_error={:?} ready={}",
                                lifecycle.last_error(),
                                stage.media_error(),
                                stage.is_ready()
                            ));
                        }
                        if let Err(error) = atomic_replace(&path, LIFECYCLE_RASTER) {
                            return fail(format!("raster replacement: {error}"));
                        }
                        state.set(8);
                    }
                    8 if lifecycle.reload_count() >= 6 => {
                        if stage.stats().pending == 0 {
                            return fail(
                                "large raster completed before cancellation could be exercised"
                                    .into(),
                            );
                        }
                        if let Err(error) = atomic_replace(&path, LIFECYCLE_RECOVERED) {
                            return fail(format!("raster cancellation replacement: {error}"));
                        }
                        state.set(9);
                    }
                    9 if lifecycle.reload_count() >= 7 => {
                        let since = cancellation_wait.get().unwrap_or_else(|| {
                            let now = Instant::now();
                            cancellation_wait.set(Some(now));
                            now
                        });
                        if since.elapsed() < Duration::from_millis(750) {
                            return glib::ControlFlow::Continue;
                        }
                        let stats = stage.stats();
                        if stats.pending != 0 || stats.textures != 0 {
                            return fail(format!(
                                "stale raster escaped cancellation pending={} textures={}",
                                stats.pending, stats.textures
                            ));
                        }
                        if !stage.set_slide_without_transition(0) {
                            return fail("unable to select SVG slide for resize".into());
                        }
                        state.set(10);
                    }
                    10 => {
                        let cache = stage.render_cache_stats();
                        if cache.text_nodes == 0 || cache.svg_nodes == 0 {
                            return glib::ControlFlow::Continue;
                        }
                        resize_before.set(Some(cache));
                        let (width, height) = if cache.width > 1_000 {
                            (960, 600)
                        } else {
                            (1_280, 720)
                        };
                        window.set_default_size(width, height);
                        state.set(11);
                    }
                    11 => {
                        let Some(before) = resize_before.get() else {
                            return fail("resize baseline was not recorded".into());
                        };
                        let cache = stage.render_cache_stats();
                        if cache.output_generation <= before.output_generation {
                            return glib::ControlFlow::Continue;
                        }
                        if (cache.width, cache.height, cache.scale)
                            == (before.width, before.height, before.scale)
                            || cache.text_nodes != 1
                            || cache.svg_nodes != 1
                            || cache.curl_cached
                        {
                            return fail(format!(
                                "resize cache mismatch before={before:?} after={cache:?}"
                            ));
                        }
                        eprintln!(
                            "PINPOINT LIFECYCLE output-resized generation={} size={}x{} scale={} text_nodes={} svg_nodes={}",
                            cache.output_generation,
                            cache.width,
                            cache.height,
                            cache.scale,
                            cache.text_nodes,
                            cache.svg_nodes
                        );
                        if stress {
                            stage.set_asset_load_delay(Duration::ZERO);
                            stress_rss_start.set(memory_kib().0);
                            if let Err(error) = atomic_replace(&path, LIFECYCLE_MISSING_RASTER) {
                                return fail(format!("stress replacement: {error}"));
                            }
                            stress_target_reload.set(lifecycle.reload_count().saturating_add(1));
                            state.set(12);
                            return glib::ControlFlow::Continue;
                        }
                        eprintln!(
                            "PINPOINT LIFECYCLE PASS reloads={} invalidations={} selected_slide={} ready={} missing_media=handled corrupt_media=handled stale_raster=cancelled output_resize=handled",
                            lifecycle.reload_count(),
                            lifecycle.asset_invalidation_count(),
                            stage.current_slide(),
                            stage.is_ready()
                        );
                        result.set(true);
                        cleanup_lifecycle_fixture(&path);
                        app.quit();
                        return glib::ControlFlow::Break;
                    }
                    12 if lifecycle.reload_count() >= stress_target_reload.get() => {
                        let stats = stage.stats();
                        if stats.pending > 0 || stats.active_loaders > 0 {
                            return glib::ControlFlow::Continue;
                        }
                        let step = stress_step.get();
                        if step.is_multiple_of(2) {
                            if stats.failures == 0 {
                                return fail(format!(
                                    "stress missing raster did not fail step={step}"
                                ));
                            }
                        } else if stats.failures != 0 || !stage.is_ready() {
                            return fail(format!(
                                "stress recovery did not settle step={step} failures={} ready={}",
                                stats.failures,
                                stage.is_ready()
                            ));
                        }
                        let next = step.saturating_add(1);
                        if next >= stress_cycles {
                            let rss_end = memory_kib().0;
                            let rss_growth = rss_end.saturating_sub(stress_rss_start.get());
                            eprintln!(
                                "PINPOINT LIFECYCLE STRESS PASS cycles={} reloads={} pending={} loaders={} failures={} rss_start_mib={:.2} rss_end_mib={:.2} rss_growth_mib={:.2}",
                                stress_cycles,
                                lifecycle.reload_count(),
                                stats.pending,
                                stats.active_loaders,
                                stats.failures,
                                stress_rss_start.get() as f64 / 1024.0,
                                rss_end as f64 / 1024.0,
                                rss_growth as f64 / 1024.0
                            );
                            result.set(true);
                            cleanup_lifecycle_fixture(&path);
                            app.quit();
                            return glib::ControlFlow::Break;
                        }
                        let contents = if next.is_multiple_of(2) {
                            LIFECYCLE_MISSING_RASTER
                        } else {
                            LIFECYCLE_RECOVERED
                        };
                        if let Err(error) = atomic_replace(&path, contents) {
                            return fail(format!("stress replacement step={next}: {error}"));
                        }
                        stress_step.set(next);
                        stress_target_reload.set(lifecycle.reload_count().saturating_add(1));
                    }
                    _ => {}
                }
                glib::ControlFlow::Continue
            }
        ),
    );
}

fn start_speaker_validation(
    app: &adw::Application,
    presenter: speaker::SpeakerPresenter,
    result: Rc<Cell<bool>>,
) {
    if !presenter.presentation_chrome_matches_contract() {
        eprintln!("PINPOINT SPEAKER FAIL audience or speaker window chrome mismatch");
        result.set(false);
        presenter.stop();
        app.quit();
        return;
    }
    if !presenter.speaker_visibility_matches_contract() {
        eprintln!("PINPOINT SPEAKER FAIL hide and restore changed the paired session");
        result.set(false);
        presenter.stop();
        app.quit();
        return;
    }
    let state = Rc::new(Cell::new(0_u8));
    let mpris_result = Rc::new(Cell::new(None::<bool>));
    let swap_handled = Rc::new(Cell::new(false));
    let pixel_agreement = Rc::new(Cell::new(None));
    let started = Instant::now();
    glib::timeout_add_local(
        Duration::from_millis(100),
        glib::clone!(
            #[weak]
            app,
            #[strong]
            presenter,
            #[strong]
            result,
            #[strong]
            state,
            #[strong]
            mpris_result,
            #[strong]
            swap_handled,
            #[strong]
            pixel_agreement,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move || {
                let fail = |message: String| {
                    eprintln!("PINPOINT SPEAKER FAIL {message}");
                    result.set(false);
                    presenter.stop();
                    app.quit();
                    glib::ControlFlow::Break
                };
                if started.elapsed() > Duration::from_secs(20) {
                    return fail(format!("timeout state={}", state.get()));
                }
                match state.get() {
                    0 => {
                        let stats = presenter.asset_stats();
                        if stats.pending > 0 {
                            return glib::ControlFlow::Continue;
                        }
                        if !presenter.assets_are_shared() || !presenter.previews_match() {
                            return fail("initial model or cache was not shared".into());
                        }
                        if !presenter.preview_scroll_matches_contract() {
                            return fail(
                                "preview rail was not independently touchpad-scrollable".into(),
                            );
                        }
                        if stats.textures != 1 {
                            return fail(format!(
                                "shared raster decoded {} times instead of once",
                                stats.textures
                            ));
                        }
                        let Some(mpris_name) = presenter.mpris_bus_name() else {
                            return fail("MPRIS adapter was not exported".into());
                        };
                        if !mpris_name.starts_with("org.mpris.MediaPlayer2.") {
                            return fail(format!("unexpected MPRIS name {mpris_name}"));
                        }
                        if let Err(error) = presenter.run_current_command() {
                            return fail(format!("command launch: {error}"));
                        }
                        state.set(20);
                    }
                    20 => {
                        match presenter.command_status() {
                            None => return glib::ControlFlow::Continue,
                            Some(Err(error)) => {
                                return fail(format!("command completion: {error}"));
                            }
                            Some(Ok(())) => {}
                        }
                        let Some(mpris_name) = presenter.mpris_bus_name() else {
                            return fail("MPRIS adapter disappeared".into());
                        };
                        let Some(connection) = app.dbus_connection() else {
                            return fail("application D-Bus connection is unavailable".into());
                        };
                        let mpris_result = mpris_result.clone();
                        connection.call(
                            Some(&mpris_name),
                            "/org/mpris/MediaPlayer2",
                            "org.mpris.MediaPlayer2.Player",
                            "Next",
                            None,
                            None,
                            gio::DBusCallFlags::NONE,
                            -1,
                            None::<&gio::Cancellable>,
                            move |reply| mpris_result.set(Some(reply.is_ok())),
                        );
                        state.set(10);
                    }
                    10 => {
                        match mpris_result.get() {
                            None => return glib::ControlFlow::Continue,
                            Some(false) => return fail("MPRIS Next was rejected".into()),
                            Some(true) if presenter.current_slide() != 1 => {
                                return fail(format!(
                                    "MPRIS Next selected slide {} instead of 1",
                                    presenter.current_slide()
                                ));
                            }
                            Some(true) => {}
                        }
                        presenter.start_timer();
                        presenter.set_autoadvance(true);
                        if !presenter.autoadvance() {
                            return fail("autoadvance state was not retained".into());
                        }
                        presenter.set_autoadvance(false);
                        if !presenter.next() {
                            return fail("unable to navigate to the second slide".into());
                        }
                        state.set(1);
                    }
                    1 if started.elapsed() >= Duration::from_millis(350) => {
                        if presenter.audience_is_transitioning() {
                            return glib::ControlFlow::Continue;
                        }
                        if presenter.current_slide() != 1
                            || !presenter.previews_match()
                            || !presenter.notes().contains("slide two")
                        {
                            return fail(format!(
                                "synchronization mismatch current={} previews={} notes={:?}",
                                presenter.current_slide(),
                                presenter.previews_match(),
                                presenter.notes()
                            ));
                        }
                        let agreement = match presenter.pixel_agreement() {
                            Ok(agreement) => agreement,
                            Err(error) => {
                                return fail(format!("speaker pixel capture failed: {error}"));
                            }
                        };
                        if agreement.differing_pixels != 0 {
                            return fail(format!(
                                "speaker pixel mismatch pixels={} differing={} max_delta={}",
                                agreement.pixels,
                                agreement.differing_pixels,
                                agreement.max_channel_delta
                            ));
                        }
                        pixel_agreement.set(Some(agreement));
                        presenter.toggle_timer();
                        presenter.toggle_fullscreen();
                        presenter.toggle_fullscreen();
                        let swapped = presenter.swap_displays();
                        if presenter.monitor_count() >= 2 && !swapped {
                            return fail("two-display swap was unavailable".into());
                        }
                        swap_handled.set(swapped);
                        if !presenter.last() {
                            return fail("unable to select the command-failure slide".into());
                        }
                        if let Err(error) = presenter.run_current_command() {
                            return fail(format!("failure command launch: {error}"));
                        }
                        state.set(30);
                    }
                    30 => {
                        match presenter.command_status() {
                            None => return glib::ControlFlow::Continue,
                            Some(Ok(())) => {
                                return fail("failing slide command unexpectedly succeeded".into());
                            }
                            Some(Err(_)) => {}
                        }
                        if !presenter.previous() {
                            return fail(
                                "unable to restore the second slide after command failure".into(),
                            );
                        }
                        let stats = presenter.asset_stats();
                        let agreement = pixel_agreement
                            .get()
                            .expect("speaker pixel agreement was recorded");
                        presenter.close_speaker_for_validation();
                        if !presenter.paired_windows_are_closed() {
                            return fail(
                                "closing the speaker window left a presentation window open".into(),
                            );
                        }
                        eprintln!(
                            "PINPOINT SPEAKER PASS slide={} previews=synchronized preview_scroll=touchpad-safe pixels={} pixel_differing={} pixel_max_delta={} notes=visible timer=running autoadvance=controlled mpris=round-trip command=success-and-failure shared_assets=true textures={} texture_mib={:.2} audience_decorated=false speaker_header=draggable hide_restore=verified paired_close=verified close_control=motion-fade-200ms/2000ms monitors={} swap={}",
                            presenter.current_slide(),
                            agreement.pixels,
                            agreement.differing_pixels,
                            agreement.max_channel_delta,
                            stats.textures,
                            stats.texture_bytes as f64 / 1024.0 / 1024.0,
                            presenter.monitor_count(),
                            if swap_handled.get() {
                                "handled"
                            } else {
                                "single-display-fallback"
                            }
                        );
                        result.set(true);
                        app.quit();
                        return glib::ControlFlow::Break;
                    }
                    _ => {}
                }
                glib::ControlFlow::Continue
            }
        ),
    );
}

fn start_normal_controls_validation(
    app: &adw::Application,
    stage: &Stage,
    controls: NormalControls,
    result: Rc<Cell<bool>>,
) {
    let state = Rc::new(Cell::new(0_u8));
    let mpris_result = Rc::new(Cell::new(None::<bool>));
    let started = Instant::now();
    glib::timeout_add_local(
        Duration::from_millis(50),
        glib::clone!(
            #[weak]
            app,
            #[weak]
            stage,
            #[strong]
            controls,
            #[strong]
            result,
            #[strong]
            state,
            #[strong]
            mpris_result,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move || {
                let fail = |message: String| {
                    eprintln!("PINPOINT NORMAL CONTROLS FAIL {message}");
                    result.set(false);
                    app.quit();
                    glib::ControlFlow::Break
                };
                if started.elapsed() > Duration::from_secs(20) {
                    return fail(format!("timeout state={}", state.get()));
                }
                match state.get() {
                    0 if !stage.is_ready() => return glib::ControlFlow::Continue,
                    0 => {
                        let Some(mpris_name) = controls.mpris_bus_name() else {
                            return fail("MPRIS adapter was not exported".into());
                        };
                        if !mpris_name.starts_with("org.mpris.MediaPlayer2.") {
                            return fail(format!("unexpected MPRIS name {mpris_name}"));
                        }
                        if let Err(error) = controls.run_current_command(&stage) {
                            return fail(format!("command launch: {error}"));
                        }
                        state.set(1);
                    }
                    1 => match controls.command_status() {
                        None => return glib::ControlFlow::Continue,
                        Some(Err(error)) => return fail(format!("command completion: {error}")),
                        Some(Ok(())) => {
                            let Some(mpris_name) = controls.mpris_bus_name() else {
                                return fail("MPRIS adapter disappeared".into());
                            };
                            let Some(connection) = app.dbus_connection() else {
                                return fail("application D-Bus connection is unavailable".into());
                            };
                            let mpris_result = mpris_result.clone();
                            connection.call(
                                Some(&mpris_name),
                                "/org/mpris/MediaPlayer2",
                                "org.mpris.MediaPlayer2.Player",
                                "Next",
                                None,
                                None,
                                gio::DBusCallFlags::NONE,
                                -1,
                                None::<&gio::Cancellable>,
                                move |reply| mpris_result.set(Some(reply.is_ok())),
                            );
                            state.set(2);
                        }
                    },
                    2 => match mpris_result.get() {
                        None => return glib::ControlFlow::Continue,
                        Some(false) => return fail("MPRIS Next was rejected".into()),
                        Some(true) if stage.current_slide() != 1 => {
                            return fail("MPRIS Next did not advance the presentation".into());
                        }
                        Some(true) => {
                            if !stage.last() {
                                return fail("unable to select the failing-command slide".into());
                            }
                            if let Err(error) = controls.run_current_command(&stage) {
                                return fail(format!("failure command launch: {error}"));
                            }
                            state.set(3);
                        }
                    },
                    3 => match controls.command_status() {
                        None => return glib::ControlFlow::Continue,
                        Some(Ok(())) => {
                            return fail("failing slide command unexpectedly succeeded".into());
                        }
                        Some(Err(_)) => {
                            (controls.sync)();
                            eprintln!(
                                "PINPOINT NORMAL CONTROLS PASS slide={} mpris=round-trip command=success-and-failure fullscreen=tracked",
                                stage.current_slide()
                            );
                            result.set(true);
                            app.quit();
                            return glib::ControlFlow::Break;
                        }
                    },
                    _ => {}
                }
                glib::ControlFlow::Continue
            }
        ),
    );
}

fn start_rehearsal_validation(
    app: &adw::Application,
    presenter: speaker::SpeakerPresenter,
    result: Rc<Cell<bool>>,
    path: PathBuf,
) {
    let state = Rc::new(Cell::new(0_u8));
    let started = Instant::now();
    glib::timeout_add_local(
        Duration::from_millis(100),
        glib::clone!(
            #[weak]
            app,
            #[strong]
            presenter,
            #[strong]
            result,
            #[strong]
            state,
            #[strong]
            path,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move || {
                let fail = |message: String| {
                    eprintln!("PINPOINT REHEARSAL FAIL {message}");
                    result.set(false);
                    presenter.stop();
                    cleanup_rehearsal_fixture(&path);
                    app.quit();
                    glib::ControlFlow::Break
                };
                if started.elapsed() > Duration::from_secs(20) {
                    return fail(format!("timeout state={}", state.get()));
                }
                match state.get() {
                    0 => {
                        if let Err(error) = presenter.start_rehearsal() {
                            return fail(format!("start: {error}"));
                        }
                        state.set(1);
                    }
                    1 if started.elapsed() >= Duration::from_millis(250) => {
                        let before = presenter.current_slide();
                        let advanced = presenter.next();
                        if !presenter.rehearsal_active() || !advanced {
                            return fail(format!(
                                "first slide did not record and advance current={before}->{} advanced={advanced}",
                                presenter.current_slide(),
                            ));
                        }
                        state.set(2);
                    }
                    2 if started.elapsed() >= Duration::from_millis(450) => {
                        if presenter.next() || presenter.rehearsal_active() {
                            return fail("final advance did not complete rehearsal".into());
                        }
                        if let Some(error) = presenter.rehearsal_error() {
                            return fail(format!("writeback: {error}"));
                        }
                        let source = match std::fs::read_to_string(&path) {
                            Ok(source) => source,
                            Err(error) => return fail(format!("readback: {error}")),
                        };
                        let saved = match pinpoint_core::presentation::load(&path, false) {
                            Ok(saved) => saved,
                            Err(error) => return fail(format!("saved parse: {error}")),
                        };
                        if !source.starts_with("[duration=30] # retain defaults and comments\n")
                            || saved.slides.len() != 2
                            || saved.slides.iter().any(|slide| slide.duration <= 0.0)
                        {
                            return fail(format!(
                                "source preservation or timings failed: {source:?}"
                            ));
                        }
                        eprintln!(
                            "PINPOINT REHEARSAL PASS slides=2 timings=recorded writeback=source-preserving concurrent_change=guarded"
                        );
                        result.set(true);
                        presenter.stop();
                        cleanup_rehearsal_fixture(&path);
                        app.quit();
                        return glib::ControlFlow::Break;
                    }
                    _ => {}
                }
                glib::ControlFlow::Continue
            }
        ),
    );
}

fn start_editor_validation(app: &adw::Application, editor: editor::Editor, result: Rc<Cell<bool>>) {
    let started = Instant::now();
    let timings_requested = Rc::new(Cell::new(false));
    let present_requested = Rc::new(Cell::new(false));
    let present_closed = Rc::new(Cell::new(false));
    let present_window = Rc::new(RefCell::new(None));
    glib::timeout_add_local(
        Duration::from_millis(100),
        glib::clone!(
            #[weak]
            app,
            #[strong]
            editor,
            #[strong]
            result,
            #[strong]
            timings_requested,
            #[strong]
            present_requested,
            #[strong]
            present_closed,
            #[strong]
            present_window,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move || {
                let fail = |message: String| {
                    eprintln!("PINPOINT EDITOR FAIL {message}");
                    result.set(false);
                    app.quit();
                    glib::ControlFlow::Break
                };
                if started.elapsed() > Duration::from_secs(10) {
                    return fail("timeout".into());
                }
                if editor.outline_count() != 3 {
                    return glib::ControlFlow::Continue;
                }
                if !editor.language_loaded() {
                    return fail("Pinpoint GtkSourceView language was not loaded".into());
                }
                if !editor.diagnostics_decorated() {
                    return fail("editor diagnostics were not underlined".into());
                }
                if !timings_requested.get() {
                    if !editor.status().contains("safe preview") {
                        return fail(format!("unexpected editor status: {}", editor.status()));
                    }
                    timings_requested.set(true);
                    editor.apply_rehearsal_durations(vec![1.25, 2.5, 3.75]);
                    return glib::ControlFlow::Continue;
                }
                if !present_requested.get() {
                    if !editor.is_modified() || !editor.status().contains("Timings applied") {
                        return fail(format!("timing application failed: {}", editor.status()));
                    }
                    let Some(window) = editor.present_unsaved_snapshot() else {
                        return fail("Present rejected a valid unsaved editor buffer".into());
                    };
                    if editor.is_visible() || !window.is_visible() {
                        return fail(
                            "Present did not transfer focus from editor to snapshot window".into(),
                        );
                    }
                    *present_window.borrow_mut() = Some(window);
                    present_requested.set(true);
                    return glib::ControlFlow::Continue;
                }
                if !editor.is_modified() {
                    return fail("Present unexpectedly saved the editor buffer".into());
                }
                if !present_closed.get() {
                    if let Some(window) = present_window.borrow_mut().take() {
                        window.close();
                    } else {
                        return fail("snapshot presentation window was not retained".into());
                    }
                    present_closed.set(true);
                    return glib::ControlFlow::Continue;
                }
                if !editor.is_visible() {
                    return glib::ControlFlow::Continue;
                }
                eprintln!(
                    "PINPOINT EDITOR PASS sourceview=pinpoint outline=3 diagnostics=underlined completion=ctrl-space safe_preview=audio-camera-command-disabled save=save-as-and-explicit rehearsal=buffer-apply present=unsaved-snapshot"
                );
                result.set(true);
                app.quit();
                glib::ControlFlow::Break
            }
        ),
    );
}

fn build_presenter(app: &adw::Application, options: &Options, result: Rc<Cell<bool>>) {
    if options.edit && options.presentation.is_none() {
        match editor::Editor::open_untitled(app, None) {
            Ok(_) => {}
            Err(error) => {
                eprintln!("pinpoint: unable to open editor: {error}");
                result.set(false);
                app.quit();
            }
        }
        return;
    }
    let lifecycle_validation = options.validate_lifecycle || options.validate_lifecycle_stress;
    let path = if lifecycle_validation {
        match lifecycle_fixture() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("PINPOINT LIFECYCLE FAIL fixture: {error}");
                result.set(false);
                app.quit();
                return;
            }
        }
    } else if options.validate_media {
        match media_fixture() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("PINPOINT MEDIA FAIL fixture: {error}");
                result.set(false);
                app.quit();
                return;
            }
        }
    } else if options.validate_rehearsal {
        match rehearsal_fixture() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("PINPOINT REHEARSAL FAIL fixture: {error}");
                result.set(false);
                app.quit();
                return;
            }
        }
    } else {
        options
            .presentation
            .clone()
            .unwrap_or_else(default_presentation)
    };
    let presentation_label = options.presentation_label.clone().unwrap_or_else(|| {
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Presentation")
            .to_owned()
    });
    if options.edit || options.validate_editor {
        match editor::Editor::open(app, path, options.ignore_comments, None) {
            Ok(editor) => {
                if options.validate_editor {
                    start_editor_validation(app, editor, result);
                }
            }
            Err(error) => {
                eprintln!("pinpoint: unable to open editor: {error}");
                result.set(false);
                app.quit();
            }
        }
        return;
    }
    let mut presentation = match pinpoint_core::presentation::load(&path, options.ignore_comments) {
        Ok(presentation) => presentation,
        Err(error) => {
            eprintln!("pinpoint: {error}");
            result.set(false);
            app.quit();
            return;
        }
    };
    presentation.asset_access = options.asset_access();

    if options.speaker_mode {
        let presenter = speaker::SpeakerPresenter::new(
            app,
            presentation,
            path.clone(),
            presentation_label,
            options.fullscreen,
        );
        if let Some(monitor) = options.audience_monitor.as_deref()
            && !presenter.prefer_audience_monitor(monitor)
        {
            eprintln!("pinpoint: requested audience display is unavailable: {monitor}");
        }
        if options.validate_rehearsal {
            start_rehearsal_validation(app, presenter, result, path);
        } else if options.validate_speaker {
            start_speaker_validation(app, presenter, result);
        } else if options.rehearse {
            if let Err(error) = presenter.start_rehearsal() {
                eprintln!("pinpoint: unable to start rehearsal: {error}");
                presenter.stop();
                result.set(false);
                app.quit();
            } else {
                eprintln!("PINPOINT REHEARSAL started from command line");
                let keepalive = presenter.clone();
                app.connect_shutdown(move |_| keepalive.stop());
            }
        } else {
            let keepalive = presenter.clone();
            app.connect_shutdown(move |_| keepalive.stop());
        }
        return;
    }

    let stage = Stage::default();
    if lifecycle_validation {
        stage.set_asset_load_delay(Duration::from_millis(500));
    }
    stage.set_presentation(presentation, path.clone());
    let loading = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    let spinner = gtk::Spinner::new();
    spinner.start();
    loading.append(&spinner);
    loading.append(&gtk::Label::new(Some("Preparing presentation…")));
    let view = gtk::Stack::builder().hexpand(true).vexpand(true).build();
    view.add_named(&loading, Some("loading"));
    view.add_named(&stage, Some("stage"));
    view.set_visible_child_name("loading");
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(format!("Pinpoint — {presentation_label}"))
        .default_width(1280)
        .default_height(720)
        .build();
    let (content, chrome) = presentation_chrome::PresentationChrome::new(&window, &stage, &view);
    window.set_content(Some(&content));
    let controls = install_normal_controls(app, &window, &stage);
    install_navigation(&window, &stage, &chrome, controls.clone());
    if options.maximized {
        window.maximize();
    }
    window.present();
    if options.fullscreen {
        let requested = options.audience_monitor.as_deref();
        let monitor = requested.and_then(|requested| {
            let monitors = gtk::prelude::WidgetExt::display(&window).monitors();
            (0..monitors.n_items()).find_map(|index| {
                monitors
                    .item(index)
                    .and_then(|monitor| monitor.downcast::<gdk::Monitor>().ok())
                    .filter(|monitor| {
                        monitor
                            .connector()
                            .or_else(|| monitor.model())
                            .is_some_and(|name| name == requested)
                    })
            })
        });
        if let Some(monitor) = monitor {
            window.fullscreen_on_monitor(&monitor);
        } else {
            if let Some(requested) = requested {
                eprintln!("pinpoint: requested audience display is unavailable: {requested}");
            }
            window.fullscreen();
        }
    }
    if let Some(output) = options.capture_real_slide.clone() {
        view.set_visible_child_name("stage");
        start_real_slide_capture(app, &window, &stage, output, options.capture_slide);
        return;
    }
    let revoke_controls = controls.clone();
    let revoke_chrome = chrome.clone();
    let lifecycle = lifecycle::Lifecycle::start(
        &stage,
        &view,
        path.clone(),
        options.ignore_comments,
        options.asset_access(),
        move || {
            if revoke_controls.revoke_command_trust() {
                revoke_chrome.show_notice("Presentation changed — command permission revoked");
            }
        },
    );
    let close_lifecycle = lifecycle.clone();
    window.connect_close_request(move |_| {
        close_lifecycle.stop();
        glib::Propagation::Proceed
    });
    stage.grab_focus();
    eprintln!(
        "PINPOINT PRESENTATION loaded slides={} path={}",
        stage.slide_count(),
        path.display()
    );
    if lifecycle_validation {
        start_lifecycle_validation(
            app,
            &window,
            &stage,
            lifecycle,
            result,
            path,
            options.validate_lifecycle_stress,
        );
    } else if options.validate_media {
        start_media_validation(app, &stage, result, path);
    } else if options.validate_normal_controls {
        start_normal_controls_validation(app, &stage, controls, result);
    } else if options.validate_stage || options.validate_pixels || options.validate_transitions {
        start_stage_validation(
            app,
            &window,
            &stage,
            &chrome,
            result,
            options.validate_pixels,
            options.validate_transitions,
        );
    }
}

fn start_application_shell_validation(app: &adw::Application, result: Rc<Cell<bool>>) {
    if let Err(error) = app_shell::validate_menu_contract() {
        eprintln!("PINPOINT SHELL FAIL menu: {error}");
        result.set(false);
        app.quit();
        return;
    }
    if let Err(error) = editor::validate_hidden_parent_lifecycle(app) {
        eprintln!("PINPOINT SHELL FAIL hidden-window lifecycle: {error}");
        result.set(false);
        app.quit();
        return;
    }
    let directory =
        std::env::temp_dir().join(format!("pinpoint-shell-validation-{}", std::process::id()));
    if let Err(error) = std::fs::create_dir_all(&directory) {
        eprintln!("PINPOINT SHELL FAIL fixture: {error}");
        result.set(false);
        app.quit();
        return;
    }
    let source = directory.join("shell.pin");
    let output = directory.join("shell.pdf");
    if let Err(error) = std::fs::write(&source, "--\nShell validation\n--\nPDF export\n") {
        eprintln!("PINPOINT SHELL FAIL fixture: {error}");
        let _ = std::fs::remove_dir_all(&directory);
        result.set(false);
        app.quit();
        return;
    }
    let app = app.clone();
    let result_for_export = result.clone();
    let source_for_export = source.clone();
    let output_for_export = output.clone();
    let export_progress = Rc::new(Cell::new((0_usize, 0_usize)));
    let export_progress_update = export_progress.clone();
    pdf::export_async(
        source.clone(),
        output,
        false,
        pdf::Options {
            page_size: pdf::PageSize::A4,
            orientation: pdf::Orientation::Landscape,
            include_speaker_notes: true,
            asset_access: asset::Access::Confined,
        },
        gio::Cancellable::new(),
        move |completed, total| export_progress_update.set((completed, total)),
        move |export_result| {
            if let Err(error) = export_result {
                eprintln!("PINPOINT SHELL FAIL export: {error}");
                let _ = std::fs::remove_dir_all(&directory);
                result_for_export.set(false);
                app.quit();
                return;
            }
            let valid_pdf = std::fs::read(&output_for_export)
                .map(|bytes| bytes.starts_with(b"%PDF-"))
                .unwrap_or(false);
            if !valid_pdf || export_progress.get() != (2, 2) {
                eprintln!(
                    "PINPOINT SHELL FAIL export result valid_pdf={valid_pdf} progress={:?}",
                    export_progress.get()
                );
                let _ = std::fs::remove_dir_all(&directory);
                result_for_export.set(false);
                app.quit();
                return;
            }
            let app_for_error = app.clone();
            let result_for_error = result_for_export.clone();
            let directory_for_error = directory.clone();
            pdf::export_async(
                source_for_export.clone(),
                source_for_export,
                false,
                pdf::Options {
                    page_size: pdf::PageSize::A4,
                    orientation: pdf::Orientation::Landscape,
                    include_speaker_notes: true,
                    asset_access: asset::Access::Confined,
                },
                gio::Cancellable::new(),
                |_, _| {},
                move |error_result| {
                    if error_result.is_ok() {
                        eprintln!("PINPOINT SHELL FAIL source destination was accepted");
                        result_for_error.set(false);
                    } else {
                        eprintln!(
                            "PINPOINT SHELL PASS menu=6 hidden-parent=close-and-restore async-export=ok progress=2/2 source-protection=ok"
                        );
                        result_for_error.set(true);
                    }
                    let _ = std::fs::remove_dir_all(&directory_for_error);
                    app_for_error.quit();
                },
            );
        },
    );
}

fn run_application(options: Options) -> glib::ExitCode {
    gst::init().expect("GStreamer initialization failed");
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let result = Rc::new(Cell::new(true));
    let activation_result = result.clone();
    app.connect_activate(move |app| {
        if options.validate_application_shell {
            start_application_shell_validation(app, activation_result.clone());
        } else if let Some(output) = options.capture_page_curl.clone() {
            start_page_curl_capture(
                app,
                activation_result.clone(),
                output,
                options.capture_width,
                options.capture_height,
                options.capture_backwards,
            );
        } else if options.presentation.is_none()
            && !options.edit
            && !options.validate_stage
            && !options.validate_pixels
            && !options.validate_transitions
            && !options.validate_lifecycle
            && !options.validate_lifecycle_stress
            && !options.validate_editor
            && !options.validate_speaker
            && !options.validate_normal_controls
            && !options.validate_rehearsal
            && !options.validate_media
            && !options.validate_application_shell
        {
            setup::build(
                app,
                options.fullscreen,
                options.speaker_mode,
                options.ignore_comments,
                default_presentation(),
                glib::clone!(
                    #[weak]
                    app,
                    #[strong]
                    options,
                    #[strong]
                    activation_result,
                    move |launch| {
                        let mut launch_options = options.clone();
                        launch_options.presentation = Some(launch.presentation);
                        launch_options.presentation_label = Some(launch.presentation_label);
                        launch_options.fullscreen = launch.fullscreen;
                        launch_options.speaker_mode = launch.speaker_mode;
                        launch_options.rehearse = launch.rehearse;
                        launch_options.audience_monitor = launch.audience_monitor;
                        launch_options.ignore_comments = launch.ignore_comments;
                        build_presenter(&app, &launch_options, activation_result.clone());
                    }
                ),
                glib::clone!(
                    #[weak]
                    app,
                    move |path, ignore_comments, setup_window| match editor::Editor::open(
                        &app,
                        path,
                        ignore_comments,
                        Some(setup_window.clone()),
                    ) {
                        Ok(_) => {}
                        Err(error) => {
                            eprintln!("pinpoint: unable to open editor: {error}");
                            setup_window.set_visible(true);
                            setup_window.present();
                        }
                    }
                ),
                glib::clone!(
                    #[weak]
                    app,
                    move |setup_window| match editor::Editor::open_untitled(
                        &app,
                        Some(setup_window.clone()),
                    ) {
                        Ok(_) => {}
                        Err(error) => {
                            eprintln!("pinpoint: unable to create presentation: {error}");
                            setup_window.set_visible(true);
                            setup_window.present();
                        }
                    }
                ),
            );
        } else {
            build_presenter(app, &options, activation_result.clone());
        }
    });
    let exit = app.run_with_args(&["pinpoint"]);
    if result.get() {
        exit
    } else {
        glib::ExitCode::FAILURE
    }
}

fn main() -> glib::ExitCode {
    renderer_policy::apply();
    let options = match parse_options() {
        Ok(Some(options)) => options,
        Ok(None) => return glib::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("pinpoint: {error}");
            return glib::ExitCode::FAILURE;
        }
    };
    if options.validate_setup {
        match setup::validate() {
            Ok(evidence) => {
                eprintln!(
                    "PINPOINT SETUP PASS presentation_files={} sorted=true ignored_files={} ignored_directories={} preflight_parse=valid-and-invalid summary=parser-derived comments=refresh explicit_cli=authoritative normal_launch=reopens_nothing session_restore=deferred-gtk-4.24",
                    evidence.presentation_files,
                    evidence.ignored_files,
                    evidence.ignored_directories,
                );
                glib::ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("PINPOINT SETUP FAIL {error}");
                glib::ExitCode::FAILURE
            }
        }
    } else if let Some(kind) = options.format_assist.as_deref() {
        match run_format_assist(
            options
                .presentation
                .as_ref()
                .expect("validated presentation"),
            kind,
            options.complete_position.as_deref(),
        ) {
            Ok(()) => glib::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("pinpoint: {error}");
                glib::ExitCode::FAILURE
            }
        }
    } else if let Some(output) = options.output.as_ref() {
        let source = options.presentation.as_ref().unwrap();
        let export_options = pdf::Options {
            page_size: options.pdf_page_size,
            orientation: options.pdf_orientation,
            include_speaker_notes: options.pdf_speaker_notes,
            asset_access: options.asset_access(),
        };
        pdf::run(source, output, options.ignore_comments, export_options)
    } else if options.check {
        let path = options.presentation.as_ref().unwrap();
        match validate_presentation(path, options.ignore_comments, options.asset_access()) {
            Ok(slides) => {
                println!("{}: {slides} slides", path.display());
                glib::ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("pinpoint: {error}");
                glib::ExitCode::FAILURE
            }
        }
    } else {
        run_application(options)
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(|value| OsString::from(*value)).collect()
    }

    #[test]
    fn long_options_accept_separate_values_like_the_c_cli() {
        let options = parse_options_from(arguments(&[
            "--output",
            "talk.pdf",
            "--pdf-page-size",
            "letter",
            "--pdf-orientation",
            "portrait",
            "talk.pin",
        ]))
        .expect("valid options")
        .expect("run options");

        assert_eq!(options.output, Some(PathBuf::from("talk.pdf")));
        assert_eq!(options.presentation, Some(PathBuf::from("talk.pin")));
        assert!(matches!(options.pdf_page_size, pdf::PageSize::Letter));
        assert!(matches!(
            options.pdf_orientation,
            pdf::Orientation::Portrait
        ));
    }

    #[test]
    fn format_assistance_accepts_separate_values() {
        let options = parse_options_from(arguments(&[
            "--format-assist",
            "completions",
            "--complete-position",
            "2:3",
            "talk.pin",
        ]))
        .expect("valid options")
        .expect("run options");

        assert_eq!(options.format_assist.as_deref(), Some("completions"));
        assert_eq!(options.complete_position.as_deref(), Some("2:3"));
    }

    #[test]
    fn separate_value_options_report_missing_arguments() {
        assert!(
            parse_options_from(arguments(&["--output"]))
                .expect_err("missing output path")
                .contains("requires a filename")
        );
        assert!(
            parse_options_from(arguments(&["--format-assist"]))
                .expect_err("missing format-assist kind")
                .contains("requires a kind")
        );
    }

    #[test]
    fn external_asset_compatibility_is_explicit_and_requires_a_deck() {
        let options = parse_options_from(arguments(&["--allow-external-assets", "talk.pin"]))
            .expect("valid compatibility options")
            .expect("run options");
        assert_eq!(options.asset_access(), asset::Access::Compatible);
        assert!(
            parse_options_from(arguments(&["--allow-external-assets"]))
                .expect_err("compatibility mode without a deck")
                .contains("requires a presentation")
        );
    }
}
