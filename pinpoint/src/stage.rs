use crate::asset_store::{AssetStore, Stats};
use crate::camera::{CameraMedia, CameraPortalEvent, CameraRequest};
use crate::media::{Media, MediaStatus};
use crate::page_curl_view::PageCurlView;
use crate::transition_renderer::{self, TransitionPlan};
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk, pango};
use pinpoint_core::presentation::{
    BackgroundType, Presentation, Slide, TextAlign, first_changed_slide,
};
use pinpoint_core::render::{Rect, background_rect, shading_rect, text_rect};
use pinpoint_core::transition::{LayerState, LegacyTransition, TransitionState, is_builtin};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Once;
use std::time::{Duration, Instant};

const CURL_PREWARM_BUDGET_BYTES: usize = 256 * 1024 * 1024;
const PRESENTATION_SHORTCUTS: &str =
    "Left Right Up Down PageUp PageDown Space Home H End F11 F B C Enter Escape Q";
const PRESENTATION_HELP: &str = "Use the arrow or page keys to change slide; Home or H for the first slide; End for the last; F11 or F for fullscreen; B to blank; C to retry camera access; Enter to review a slide command; and Escape or Q to quit.";
static REDUCED_MOTION_REPORTED: Once = Once::new();

#[derive(Clone, Copy)]
struct Transition {
    previous: usize,
    backwards: bool,
    started: Instant,
    duration: Duration,
    generation: u64,
}

struct CurlCache {
    previous_index: usize,
    current_index: usize,
    width: f32,
    height: f32,
    scale: i32,
    previous: gdk::Texture,
    current: gdk::Texture,
}

#[derive(Clone)]
struct TextNodes {
    shading: gsk::RenderNode,
    foreground: gsk::RenderNode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OutputKey {
    width: i32,
    height: i32,
    scale: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderCacheStats {
    pub output_generation: u64,
    pub width: i32,
    pub height: i32,
    pub scale: i32,
    pub text_nodes: usize,
    pub svg_nodes: usize,
    pub curl_cached: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PixelAgreement {
    pub pixels: u64,
    pub differing_pixels: u64,
    pub max_channel_delta: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetProblemSummary {
    pub message: String,
    pub details: String,
}

#[derive(Default)]
struct State {
    presentation: Option<Rc<Presentation>>,
    path: Option<PathBuf>,
    asset_access: pinpoint_core::asset::Access,
    accessible_context: String,
    current: usize,
    blank: bool,
    transition: Option<Transition>,
    generation: u64,
    transition_generation: u64,
    output_generation: u64,
    output: Option<OutputKey>,
    assets: AssetStore,
    owns_assets: bool,
    suppress_media: bool,
    media_enabled: bool,
    audio_enabled: bool,
    media_looping: bool,
    text_nodes: HashMap<(usize, i32, i32), TextNodes>,
    svg_nodes: HashMap<(usize, i32, i32), gsk::RenderNode>,
    media: HashMap<PathBuf, Rc<Media>>,
    active_media: Option<PathBuf>,
    media_error: Option<String>,
    camera_enabled: bool,
    camera_media: Option<Rc<CameraMedia>>,
    camera_request: Option<CameraRequest>,
    camera_request_attempted: bool,
    camera_retry_pending: bool,
    camera_error: Option<String>,
    media_picture: Option<gtk::Picture>,
    media_offload: Option<gtk::GraphicsOffload>,
    offloaded_media: Option<PathBuf>,
    allocated_offloaded_media: Option<PathBuf>,
    page_curl_frames: u64,
    page_curl_setup_worst_ms: f64,
    page_curl_worst_ms: f64,
    page_curl_snapshot_intervals_ms: Vec<f64>,
    curl_timing_generation: u64,
    curl_last_frame: Option<Instant>,
    curl_cache: Vec<CurlCache>,
    curl_prewarm_scheduled: bool,
    curl_view: Option<PageCurlView>,
    legacy_transitions: HashMap<String, Option<Rc<LegacyTransition>>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AccessibilityText {
    label: String,
    description: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CameraPortalState {
    Idle,
    Pending,
    Denied,
    Error,
    Ready,
}

fn camera_portal_state(
    has_media: bool,
    has_request: bool,
    attempted: bool,
    has_error: bool,
) -> CameraPortalState {
    if has_media {
        CameraPortalState::Ready
    } else if has_request {
        CameraPortalState::Pending
    } else if has_error {
        CameraPortalState::Error
    } else if attempted {
        CameraPortalState::Denied
    } else {
        CameraPortalState::Idle
    }
}

fn camera_status_message(state: CameraPortalState) -> Option<(&'static str, &'static str)> {
    match state {
        CameraPortalState::Ready => None,
        CameraPortalState::Idle => Some((
            "Camera access needed",
            "Activate this presentation to request access through the desktop portal.",
        )),
        CameraPortalState::Pending => Some((
            "Camera permission requested",
            "Approve the desktop camera request to continue.",
        )),
        CameraPortalState::Denied => Some((
            "Camera access was denied",
            "Press C to request access again.",
        )),
        CameraPortalState::Error => Some((
            "Camera is unavailable",
            "Press C to retry. Technical details are available in the terminal.",
        )),
    }
}

fn slide_accessible_text(slide: &Slide) -> Option<String> {
    let text = slide.text.as_deref()?.trim();
    if text.is_empty() {
        return None;
    }
    let text = if slide.use_markup {
        pango::parse_markup(text, '\0')
            .map(|(_, plain, _)| plain.to_string())
            .unwrap_or_else(|_| text.to_owned())
    } else {
        text.to_owned()
    };
    (!text.trim().is_empty()).then(|| text.trim().to_owned())
}

fn accessibility_text(
    presentation: Option<&Presentation>,
    current: usize,
    blank: bool,
    context: &str,
    camera_state: CameraPortalState,
) -> AccessibilityText {
    let context = if context.is_empty() {
        "Presentation slide"
    } else {
        context
    };
    let Some(presentation) = presentation else {
        return AccessibilityText {
            label: context.to_owned(),
            description: "No presentation loaded".into(),
        };
    };
    let count = presentation.slides.len();
    let label = format!("{context} {} of {count}", current.saturating_add(1));
    if blank {
        return AccessibilityText {
            label,
            description: "Blank screen".into(),
        };
    }
    let slide = presentation.slides.get(current);
    let audience_text = slide.and_then(slide_accessible_text);
    let visual_description = slide
        .and_then(|slide| slide.visual_description.as_deref())
        .map(str::trim)
        .filter(|description| !description.is_empty());
    let mut description = match (audience_text, visual_description) {
        (Some(audience), Some(visual)) => format!("{audience}\nVisual description: {visual}"),
        (None, Some(visual)) => format!("Visual description: {visual}"),
        (Some(audience), None) => audience,
        (None, None) => "Slide has no audience text".into(),
    };
    if slide.is_some_and(|slide| slide.background_type == BackgroundType::Camera)
        && let Some((heading, detail)) = camera_status_message(camera_state)
    {
        description.push_str(&format!("\n{heading}. {detail}"));
    }
    AccessibilityText { label, description }
}

fn motion_duration(
    duration: Duration,
    animations_enabled: bool,
    reduced_motion: gtk::ReducedMotion,
) -> Duration {
    if !animations_enabled || reduced_motion == gtk::ReducedMotion::Reduce {
        Duration::ZERO
    } else {
        duration
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Stage {
        pub(super) state: RefCell<State>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Stage {
        const NAME: &'static str = "PinpointRustStage";
        type Type = super::Stage;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Stage {
        fn dispose(&self) {
            let mut state = self.state.borrow_mut();
            state.media.clear();
            state.active_media = None;
            state.camera_request = None;
            state.camera_media = None;
            if let Some(offload) = state.media_offload.take() {
                offload.set_child(gtk::Widget::NONE);
                offload.unparent();
            }
            if let Some(curl_view) = state.curl_view.take() {
                curl_view.clear();
                curl_view.widget().unparent();
            }
            state.media_picture = None;
            state.offloaded_media = None;
            state.allocated_offloaded_media = None;
            state.presentation = None;
            state.text_nodes.clear();
            state.svg_nodes.clear();
            if state.owns_assets {
                state.assets.clear();
            }
        }
    }

    impl WidgetImpl for Stage {
        fn map(&self) {
            self.parent_map();
            let widget = self.obj();
            widget.update_active_playback(true);
            widget.prepare_camera();
        }

        fn unmap(&self) {
            let widget = self.obj();
            widget.update_active_playback(false);
            self.parent_unmap();
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            let widget = self.obj();
            allocate_media_offload(
                &widget,
                &mut self.state.borrow_mut(),
                width as f32,
                height as f32,
            );
            if let Some(curl_view) = self.state.borrow().curl_view.as_ref() {
                curl_view.widget().allocate(width, height, baseline, None);
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            widget.sync_media_offload();
            let width = widget.width() as f32;
            let height = widget.height() as f32;
            let mut state = self.state.borrow_mut();
            let output = OutputKey {
                width: widget.width(),
                height: widget.height(),
                scale: widget.scale_factor(),
            };
            let output_changed = state.output != Some(output);
            if output_changed {
                state.output = Some(output);
                state.output_generation = state.output_generation.wrapping_add(1);
                state.text_nodes.clear();
                state.svg_nodes.clear();
                state.curl_cache.clear();
                state.curl_prewarm_scheduled = false;
                if let Some(curl_view) = state.curl_view.as_ref() {
                    curl_view.clear();
                }
            }
            snapshot_stage(&widget, snapshot, &mut state, width, height);
            drop(state);
            if output_changed {
                widget.schedule_curl_prewarm();
            }
        }
    }
}

glib::wrapper! {
    pub struct Stage(ObjectSubclass<imp::Stage>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for Stage {
    fn default() -> Self {
        let stage: Self = glib::Object::builder()
            .property("hexpand", true)
            .property("vexpand", true)
            .property("focusable", true)
            .build();
        stage.set_overflow(gtk::Overflow::Hidden);
        stage.set_accessible_role(gtk::AccessibleRole::Group);
        stage.update_property(&[
            gtk::accessible::Property::KeyShortcuts(PRESENTATION_SHORTCUTS),
            gtk::accessible::Property::Description(PRESENTATION_HELP),
        ]);
        let picture = gtk::Picture::new();
        picture.set_content_fit(gtk::ContentFit::Fill);
        picture.set_can_shrink(true);
        let offload = gtk::GraphicsOffload::new(Some(&picture));
        offload.set_enabled(gtk::GraphicsOffloadEnabled::Enabled);
        offload.set_visible(false);
        offload.set_parent(&stage);
        let curl_view = PageCurlView::new();
        // Keep the GLArea mapped so its context is realized before texture
        // prewarming. The custom Stage snapshot only includes this child for
        // page-curl frames, so it does not draw outside a transition.
        curl_view.widget().set_parent(&stage);
        {
            let mut state = stage.imp().state.borrow_mut();
            state.owns_assets = true;
            state.media_enabled = true;
            state.audio_enabled = true;
            state.media_looping = true;
            state.camera_enabled = true;
            state.accessible_context = "Presentation slide".into();
            state.media_picture = Some(picture);
            state.media_offload = Some(offload);
            state.curl_view = Some(curl_view);
        }
        stage.update_accessibility();
        stage
    }
}

impl Stage {
    pub fn with_shared_assets(assets: AssetStore, suppress_media: bool) -> Self {
        let stage = Self::default();
        {
            let mut state = stage.imp().state.borrow_mut();
            state.assets = assets;
            state.owns_assets = false;
            state.suppress_media = suppress_media;
        }
        stage
    }

    pub fn set_presentation(&self, presentation: Presentation, path: PathBuf) {
        self.set_presentation_at(presentation, path, 0);
    }

    pub fn set_presentation_at(
        &self,
        presentation: Presentation,
        path: PathBuf,
        initial_slide: usize,
    ) {
        self.imp().state.borrow().assets.clear();
        self.set_shared_presentation_at(Rc::new(presentation), path, initial_slide);
    }

    pub fn set_shared_presentation_at(
        &self,
        presentation: Rc<Presentation>,
        path: PathBuf,
        initial_slide: usize,
    ) {
        {
            let mut state = self.imp().state.borrow_mut();
            state.generation = state.generation.wrapping_add(1);
            state.transition_generation = state.transition_generation.wrapping_add(1);
            let current = initial_slide.min(presentation.slides.len().saturating_sub(1));
            state.asset_access = presentation.asset_access;
            state.presentation = Some(presentation);
            state.path = Some(path);
            state.current = current;
            state.blank = false;
            state.transition = None;
            state.text_nodes.clear();
            state.svg_nodes.clear();
            state.media.clear();
            state.active_media = None;
            state.media_error = None;
            state.camera_request = None;
            state.camera_media = None;
            state.camera_request_attempted = false;
            state.camera_retry_pending = false;
            state.camera_error = None;
            state.offloaded_media = None;
            state.allocated_offloaded_media = None;
            state.page_curl_frames = 0;
            state.page_curl_setup_worst_ms = 0.0;
            state.page_curl_worst_ms = 0.0;
            state.page_curl_snapshot_intervals_ms.clear();
            state.curl_timing_generation = 0;
            state.curl_last_frame = None;
            state.curl_cache.clear();
            state.legacy_transitions.clear();
            state.curl_prewarm_scheduled = false;
            if let Some(curl_view) = state.curl_view.as_ref() {
                curl_view.clear();
            }
        }
        self.update_accessibility();
        self.prepare_working_set();
        self.schedule_curl_prewarm();
        self.queue_draw();
    }

    pub fn reload_presentation(&self, presentation: Presentation, path: PathBuf) -> usize {
        let initial_slide = self
            .imp()
            .state
            .borrow()
            .presentation
            .as_deref()
            .map_or(0, |old| first_changed_slide(old, &presentation));
        self.set_presentation_at(presentation, path, initial_slide);
        initial_slide
    }

    pub fn slide_count(&self) -> usize {
        self.imp()
            .state
            .borrow()
            .presentation
            .as_ref()
            .map_or(0, |presentation| presentation.slides.len())
    }

    pub fn generation(&self) -> u64 {
        self.imp().state.borrow().generation
    }

    pub fn current_slide(&self) -> usize {
        self.imp().state.borrow().current
    }

    pub fn set_accessible_context(&self, context: &str) {
        let context = context.trim();
        if context.is_empty() {
            return;
        }
        let changed = {
            let mut state = self.imp().state.borrow_mut();
            if state.accessible_context == context {
                false
            } else {
                state.accessible_context = context.into();
                true
            }
        };
        if changed {
            self.update_accessibility();
        }
    }

    pub fn pixel_agreement_with(
        &self,
        other: &Stage,
        width: i32,
        height: i32,
    ) -> Result<PixelAgreement, String> {
        if width <= 0 || height <= 0 {
            return Err("pixel comparison requires a positive output size".into());
        }
        let (ours, our_stride) = self.snapshot_rgba(width, height)?;
        let (theirs, their_stride) = other.snapshot_rgba(width, height)?;
        let mut agreement = PixelAgreement::default();
        for y in 0..height as usize {
            for x in 0..width as usize {
                let ours = &ours[y * our_stride + x * 4..][..4];
                let theirs = &theirs[y * their_stride + x * 4..][..4];
                let delta = ours
                    .iter()
                    .zip(theirs)
                    .map(|(left, right)| left.abs_diff(*right))
                    .max()
                    .unwrap_or(0);
                agreement.max_channel_delta = agreement.max_channel_delta.max(delta);
                agreement.pixels += 1;
                if delta != 0 {
                    agreement.differing_pixels += 1;
                }
            }
        }
        Ok(agreement)
    }

    fn snapshot_rgba(&self, width: i32, height: i32) -> Result<(glib::Bytes, usize), String> {
        let snapshot = gtk::Snapshot::new();
        {
            let mut state = self.imp().state.borrow_mut();
            snapshot_stage(self, &snapshot, &mut state, width as f32, height as f32);
        }
        let node = snapshot
            .to_node()
            .ok_or_else(|| "stage snapshot was empty".to_owned())?;
        let native = self
            .native()
            .ok_or_else(|| "stage has no GtkNative".to_owned())?;
        let renderer = native
            .renderer()
            .ok_or_else(|| "stage has no GSK renderer".to_owned())?;
        let viewport = graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
        let texture = renderer.render_texture(&node, Some(&viewport));
        if texture.width() != width || texture.height() != height {
            return Err(format!(
                "stage snapshot size mismatch: expected {width}x{height}, got {}x{}",
                texture.width(),
                texture.height()
            ));
        }
        let mut downloader = gdk::TextureDownloader::new(&texture);
        downloader.set_format(gdk::MemoryFormat::R8g8b8a8Premultiplied);
        Ok(downloader.download_bytes())
    }

    pub fn current_command(&self) -> Option<String> {
        let state = self.imp().state.borrow();
        state
            .presentation
            .as_ref()
            .and_then(|presentation| presentation.slides.get(state.current))
            .and_then(|slide| slide.command.clone())
    }

    pub fn is_blank(&self) -> bool {
        self.imp().state.borrow().blank
    }

    pub fn stats(&self) -> Stats {
        self.imp().state.borrow().assets.stats()
    }

    pub fn shares_assets_with(&self, other: &Self) -> bool {
        self.imp()
            .state
            .borrow()
            .assets
            .ptr_eq(&other.imp().state.borrow().assets)
    }

    pub fn render_cache_stats(&self) -> RenderCacheStats {
        let state = self.imp().state.borrow();
        let output = state.output.unwrap_or(OutputKey {
            width: 0,
            height: 0,
            scale: 0,
        });
        RenderCacheStats {
            output_generation: state.output_generation,
            width: output.width,
            height: output.height,
            scale: output.scale,
            text_nodes: state.text_nodes.len(),
            svg_nodes: state.svg_nodes.len(),
            curl_cached: !state.curl_cache.is_empty(),
        }
    }

    pub fn set_asset_load_delay(&self, delay: Duration) {
        self.imp().state.borrow().assets.set_load_delay(delay);
    }

    pub fn set_asset_access(&self, access: pinpoint_core::asset::Access) {
        self.imp().state.borrow_mut().asset_access = access;
    }

    pub fn is_ready(&self) -> bool {
        let state = self.imp().state.borrow();
        state.assets.stats().pending == 0 && state.media.values().all(|media| media.is_prepared())
    }

    pub fn monitored_assets(&self) -> Vec<PathBuf> {
        let state = self.imp().state.borrow();
        let Some(presentation) = state.presentation.as_ref() else {
            return Vec::new();
        };
        let mut assets = Vec::new();
        for slide in &presentation.slides {
            if matches!(
                slide.background_type,
                BackgroundType::Image | BackgroundType::Svg | BackgroundType::Video
            ) && let Some(background) = slide.background.as_deref()
                && let Some(path) =
                    resolve_asset(state.path.as_deref(), background, state.asset_access)
                && !assets.contains(&path)
            {
                assets.push(path);
            }
            if !slide.transition.is_empty() && !is_builtin(&slide.transition) {
                let filename = if slide.transition.ends_with(".json") {
                    slide.transition.clone()
                } else {
                    format!("{}.json", slide.transition)
                };
                if let Some(path) =
                    resolve_asset(state.path.as_deref(), &filename, state.asset_access)
                    && !assets.contains(&path)
                {
                    assets.push(path);
                }
            }
        }
        assets
    }

    pub fn invalidate_asset(&self, path: &Path) {
        {
            let mut state = self.imp().state.borrow_mut();
            state.assets.invalidate(path);
            state.svg_nodes.clear();
            state.curl_cache.clear();
            state.legacy_transitions.clear();
            state.curl_prewarm_scheduled = false;
            state.media_error = None;
            state.media.remove(path);
            if state.active_media.as_deref() == Some(path) {
                state.active_media = None;
            }
        }
        self.prepare_working_set();
        self.schedule_curl_prewarm();
        self.queue_draw();
    }

    pub fn media_error(&self) -> Option<String> {
        let state = self.imp().state.borrow();
        state
            .media_error
            .clone()
            .or_else(|| state.camera_error.clone())
            .or_else(|| state.camera_media.as_ref().and_then(|media| media.error()))
            .or_else(|| state.media.values().find_map(|media| media.error()))
    }

    pub fn asset_problem_summary(&self) -> Option<AssetProblemSummary> {
        let state = self.imp().state.borrow();
        let presentation = state.presentation.as_ref()?;
        let mut problems = Vec::<(PathBuf, String, Vec<usize>)>::new();
        for (index, slide) in presentation.slides.iter().enumerate() {
            if !matches!(
                slide.background_type,
                BackgroundType::Image | BackgroundType::Svg | BackgroundType::Video
            ) {
                continue;
            }
            let Some(background) = slide.background.as_deref() else {
                continue;
            };
            let Some(path) = resolve_asset(state.path.as_deref(), background, state.asset_access)
            else {
                add_asset_problem(
                    &mut problems,
                    PathBuf::from(background),
                    "outside the presentation folder or unsupported".into(),
                    index + 1,
                );
                continue;
            };
            let error = if !path.exists() {
                Some("missing or inaccessible".into())
            } else if let Some(error) = state.assets.failure(&path) {
                Some(error)
            } else if slide.background_type == BackgroundType::Video {
                state.media.get(&path).and_then(|media| media.error())
            } else {
                None
            };
            if let Some(error) = error {
                add_asset_problem(&mut problems, path, error, index + 1);
            }
        }
        if problems.is_empty() {
            return None;
        }
        let mut slides = problems
            .iter()
            .flat_map(|(_, _, slides)| slides.iter().copied())
            .collect::<Vec<_>>();
        slides.sort_unstable();
        slides.dedup();
        let count = problems.len();
        let details = problems
            .iter()
            .map(|(path, error, slides)| {
                format!(
                    "{} — {error} (slide{})",
                    path.file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy(),
                    format_slide_numbers(slides)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        Some(AssetProblemSummary {
            message: format!(
                "{count} asset problem{} · slide{}",
                if count == 1 { "" } else { "s" },
                format_slide_numbers(&slides)
            ),
            details,
        })
    }

    pub fn retry_failed_assets(&self) {
        let paths = problem_paths(&self.imp().state.borrow());
        {
            let mut state = self.imp().state.borrow_mut();
            for path in &paths {
                state.assets.invalidate(path);
                state.media.remove(path);
            }
            state.media_error = None;
            state.svg_nodes.clear();
        }
        self.prepare_working_set();
        self.queue_draw();
    }

    pub fn presentation_folder(&self) -> Option<PathBuf> {
        self.imp()
            .state
            .borrow()
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
    }

    pub fn pending_media(&self) -> usize {
        self.imp()
            .state
            .borrow()
            .media
            .values()
            .filter(|media| !media.is_prepared())
            .count()
    }

    pub fn prepared_media(&self) -> usize {
        self.imp().state.borrow().media.len()
    }

    pub fn active_media_status(&self) -> Option<MediaStatus> {
        let state = self.imp().state.borrow();
        state
            .active_media
            .as_ref()
            .and_then(|path| state.media.get(path))
            .map(|media| media.status())
    }

    pub fn media_offload_configured(&self) -> bool {
        self.imp().state.borrow().offloaded_media.is_some()
    }

    pub fn is_media_enabled(&self) -> bool {
        self.imp().state.borrow().media_enabled
    }

    pub fn is_audio_enabled(&self) -> bool {
        self.imp().state.borrow().audio_enabled
    }

    pub fn camera_portal_state(&self) -> &'static str {
        let state = self.imp().state.borrow();
        match camera_portal_state(
            state.camera_media.is_some(),
            state.camera_request.is_some(),
            state.camera_request_attempted,
            state.camera_error.is_some(),
        ) {
            CameraPortalState::Ready => "ready",
            CameraPortalState::Pending => "pending",
            CameraPortalState::Denied => "denied",
            CameraPortalState::Error => "error",
            CameraPortalState::Idle => "idle",
        }
    }

    pub fn retry_camera(&self) -> bool {
        let retry = {
            let mut state = self.imp().state.borrow_mut();
            let on_camera_slide = state
                .presentation
                .as_ref()
                .and_then(|presentation| presentation.slides.get(state.current))
                .is_some_and(|slide| slide.background_type == BackgroundType::Camera);
            if !state.media_enabled
                || !state.camera_enabled
                || state.suppress_media
                || state.blank
                || !on_camera_slide
                || state.camera_media.is_some()
                || state.camera_request.is_some()
            {
                false
            } else {
                state.camera_request_attempted = false;
                state.camera_error = None;
                true
            }
        };
        if retry {
            self.prepare_camera();
            self.update_accessibility();
            self.queue_draw();
        }
        retry
    }

    pub fn seek_active_media(&self, position: Duration) -> Result<(), String> {
        let state = self.imp().state.borrow();
        let Some(media) = state
            .active_media
            .as_ref()
            .and_then(|path| state.media.get(path))
        else {
            return Err("no active media".to_owned());
        };
        media.seek(position)
    }

    pub fn set_media_looping(&self, looping: bool) {
        let mut state = self.imp().state.borrow_mut();
        state.media_looping = looping;
        for media in state.media.values() {
            media.set_looping(looping);
        }
    }

    pub fn set_media_enabled(&self, enabled: bool) {
        {
            let mut state = self.imp().state.borrow_mut();
            if state.media_enabled == enabled {
                return;
            }
            state.media_enabled = enabled;
            state.media.clear();
            state.active_media = None;
            state.media_error = None;
            if !enabled {
                state.camera_request = None;
                state.camera_media = None;
                state.camera_request_attempted = false;
            }
        }
        self.prepare_working_set();
        self.queue_draw();
    }

    pub fn set_audio_enabled(&self, enabled: bool) {
        {
            let mut state = self.imp().state.borrow_mut();
            if state.audio_enabled == enabled {
                return;
            }
            state.audio_enabled = enabled;
            state.media.clear();
            state.active_media = None;
            state.media_error = None;
        }
        self.prepare_working_set();
        self.queue_draw();
    }

    pub fn set_camera_enabled(&self, enabled: bool) {
        {
            let mut state = self.imp().state.borrow_mut();
            if state.camera_enabled == enabled {
                return;
            }
            state.camera_enabled = enabled;
            state.camera_request = None;
            if !enabled {
                state.camera_media = None;
            }
            state.camera_request_attempted = false;
            state.camera_error = None;
        }
        self.prepare_camera();
        self.queue_draw();
    }

    pub fn is_transitioning(&self) -> bool {
        self.imp().state.borrow().transition.is_some()
    }

    pub fn page_curl_frames(&self) -> u64 {
        self.imp().state.borrow().page_curl_frames
    }

    pub fn page_curl_worst_ms(&self) -> f64 {
        self.imp().state.borrow().page_curl_worst_ms
    }

    pub fn page_curl_setup_worst_ms(&self) -> f64 {
        self.imp().state.borrow().page_curl_setup_worst_ms
    }

    pub fn page_curl_snapshot_p95_ms(&self) -> f64 {
        let state = self.imp().state.borrow();
        if state.page_curl_snapshot_intervals_ms.is_empty() {
            return 0.0;
        }
        let mut intervals = state.page_curl_snapshot_intervals_ms.clone();
        intervals.sort_by(f64::total_cmp);
        let index = ((intervals.len() as f64 * 0.95).ceil() as usize)
            .saturating_sub(1)
            .min(intervals.len() - 1);
        intervals[index]
    }

    pub fn curl_prewarm_ready(&self) -> bool {
        let pair = {
            let state = self.imp().state.borrow();
            state.presentation.as_ref().and_then(|presentation| {
                let previous = state.current;
                let current = previous.saturating_add(1);
                (current < presentation.slides.len()).then_some((previous, current))
            })
        };
        pair.is_none_or(|(previous, current)| self.curl_pair_ready(previous, current, false))
    }

    pub fn curl_pair_ready(&self, previous: usize, current: usize, backwards: bool) -> bool {
        let state = self.imp().state.borrow();
        let Some(presentation) = state.presentation.as_ref() else {
            return true;
        };
        if !curl_pair_is_static(presentation, previous, current, backwards) {
            return true;
        }
        let Some(output) = state.output else {
            return false;
        };
        curl_cache_capacity(output) == 0
            || curl_cache_pair(&state.curl_cache, previous, current, output).is_some()
    }

    pub fn curl_prewarm_status(&self) -> String {
        let state = self.imp().state.borrow();
        let output = state.output.map_or_else(
            || "none".to_owned(),
            |output| format!("{}x{}@{}", output.width, output.height, output.scale),
        );
        let cache = state
            .curl_cache
            .iter()
            .map(|cache| {
                format!(
                    "{}<->{} {}x{}@{}",
                    cache.previous_index,
                    cache.current_index,
                    cache.width,
                    cache.height,
                    cache.scale
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("current={} output={output} cache={cache}", state.current)
    }

    pub fn next(&self) -> bool {
        let target = self.current_slide().saturating_add(1);
        self.go_to(target)
    }

    pub fn previous(&self) -> bool {
        let current = self.current_slide();
        if current == 0 {
            false
        } else {
            self.go_to(current - 1)
        }
    }

    pub fn first(&self) -> bool {
        self.go_to(0)
    }

    pub fn last(&self) -> bool {
        let count = self.slide_count();
        count > 0 && self.go_to(count - 1)
    }

    pub fn set_slide_without_transition(&self, slide: usize) -> bool {
        let mut state = self.imp().state.borrow_mut();
        let Some(presentation) = state.presentation.as_ref() else {
            return false;
        };
        if slide >= presentation.slides.len() {
            return false;
        }
        state.current = slide;
        state.transition = None;
        state.curl_cache.clear();
        if let Some(curl_view) = state.curl_view.as_ref() {
            curl_view.clear();
            curl_view.widget().set_visible(false);
        }
        drop(state);
        self.update_accessibility();
        self.prepare_working_set();
        self.schedule_curl_prewarm();
        self.queue_draw();
        true
    }

    fn go_to(&self, target: usize) -> bool {
        let settings = self.settings();
        let animations_enabled = settings.is_gtk_enable_animations();
        let reduced_motion = settings.gtk_interface_reduced_motion();
        let motion_reduced = !animations_enabled || reduced_motion == gtk::ReducedMotion::Reduce;
        let mut generation = None;
        {
            let mut state = self.imp().state.borrow_mut();
            let Some(presentation) = state.presentation.clone() else {
                return false;
            };
            if target >= presentation.slides.len() || target == state.current {
                return false;
            }
            let previous = state.current;
            let backwards = target < previous;
            let old_legacy = legacy_transition(&mut state, &presentation.slides[previous]);
            let new_legacy = legacy_transition(&mut state, &presentation.slides[target]);
            let old_duration = transition_renderer::duration_with_legacy(
                &presentation.slides[previous],
                false,
                backwards,
                old_legacy.as_deref(),
            );
            let new_duration = transition_renderer::duration_with_legacy(
                &presentation.slides[target],
                true,
                backwards,
                new_legacy.as_deref(),
            );
            let duration = motion_duration(
                old_duration.max(new_duration),
                animations_enabled,
                reduced_motion,
            );
            state.current = target;
            state.transition_generation = state.transition_generation.wrapping_add(1);
            if duration.is_zero() {
                state.transition = None;
                if let Some(curl_view) = state.curl_view.as_ref() {
                    curl_view.clear();
                }
            } else {
                let transition_generation = state.transition_generation;
                state.transition = Some(Transition {
                    previous,
                    backwards,
                    started: Instant::now(),
                    duration,
                    generation: transition_generation,
                });
                generation = Some(transition_generation);
            }
        }
        if generation.is_none() && motion_reduced {
            REDUCED_MOTION_REPORTED.call_once(|| {
                eprintln!("Reduced motion is enabled; slide transitions will complete immediately");
            });
        }
        self.update_accessibility();
        self.prepare_working_set();
        if let Some(generation) = generation {
            self.start_transition_tick(generation);
        } else {
            self.schedule_curl_prewarm();
        }
        self.queue_draw();
        true
    }

    fn start_transition_tick(&self, generation: u64) {
        let weak = self.downgrade();
        self.add_tick_callback(move |_, _| {
            let Some(stage) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let mut state = stage.imp().state.borrow_mut();
            let active = state.transition.is_some_and(|transition| {
                transition.generation == generation
                    && transition.started.elapsed() < transition.duration
            });
            if active {
                drop(state);
                stage.sync_media_offload();
                stage.queue_draw();
                glib::ControlFlow::Continue
            } else {
                if state
                    .transition
                    .is_some_and(|transition| transition.generation == generation)
                {
                    state.transition = None;
                    if let Some(curl_view) = state.curl_view.as_ref() {
                        curl_view.hide();
                    }
                }
                drop(state);
                stage.schedule_curl_prewarm();
                stage.queue_draw();
                glib::ControlFlow::Break
            }
        });
    }

    fn schedule_curl_prewarm(&self) {
        if self.curl_prewarm_pairs_ready() {
            return;
        }
        let should_schedule = {
            let mut state = self.imp().state.borrow_mut();
            if state.curl_prewarm_scheduled || state.transition.is_some() {
                false
            } else {
                state.curl_prewarm_scheduled = true;
                true
            }
        };
        if !should_schedule {
            return;
        }
        let weak = self.downgrade();
        glib::timeout_add_local_once(Duration::from_millis(25), move || {
            if let Some(stage) = weak.upgrade() {
                stage.prewarm_curl_pairs();
            }
        });
    }

    fn curl_prewarm_pairs_ready(&self) -> bool {
        let state = self.imp().state.borrow();
        let Some(presentation) = state.presentation.as_ref() else {
            return true;
        };
        let Some(output) = state.output else {
            return false;
        };
        let capacity = curl_cache_capacity(output);
        if capacity == 0 {
            return true;
        }
        let current = state.current;
        let forward_ready = current.saturating_add(1) >= presentation.slides.len()
            || !curl_pair_is_static(presentation, current, current + 1, false)
            || curl_cache_pair(&state.curl_cache, current, current + 1, output).is_some();
        let backward_ready = capacity <= 1
            || current == 0
            || !curl_pair_is_static(presentation, current, current - 1, true)
            || curl_cache_pair(&state.curl_cache, current, current - 1, output).is_some();
        forward_ready && backward_ready
    }

    fn prewarm_curl_pairs(&self) {
        let mut retry = false;
        let mut warmed = Vec::new();
        {
            let mut state = self.imp().state.borrow_mut();
            state.curl_prewarm_scheduled = false;
            if state.transition.is_some()
                || state.assets.stats().pending > 0
                || state.media.values().any(|media| !media.is_prepared())
            {
                retry = true;
            } else if let (Some(presentation), Some(output)) =
                (state.presentation.clone(), state.output)
            {
                let capacity = curl_cache_capacity(output);
                let current = state.current;
                let mut pairs = Vec::new();
                if current.saturating_add(1) < presentation.slides.len()
                    && curl_pair_is_static(&presentation, current, current + 1, false)
                {
                    pairs.push((current, current + 1, false));
                }
                if capacity > 1
                    && current > 0
                    && curl_pair_is_static(&presentation, current, current - 1, true)
                {
                    pairs.push((current, current - 1, true));
                }
                for (previous, target, backwards) in pairs {
                    let transition = Transition {
                        previous,
                        backwards,
                        started: Instant::now(),
                        duration: Duration::ZERO,
                        generation: 0,
                    };
                    if let Some((previous_texture, current_texture)) = page_curl_slide_textures(
                        self,
                        &mut state,
                        &presentation,
                        transition,
                        target,
                        output.width as f32,
                        output.height as f32,
                    ) {
                        warmed.push((previous, target, previous_texture, current_texture));
                    } else {
                        retry = true;
                        break;
                    }
                }
            } else {
                retry = true;
            }
        }
        if !warmed.is_empty() {
            if let Some(curl_view) = self.imp().state.borrow().curl_view.as_ref() {
                let pairs = warmed
                    .iter()
                    .map(|(_, _, previous, current)| (previous.clone(), current.clone()))
                    .collect();
                curl_view.prewarm_textures(pairs);
                let directions = warmed
                    .iter()
                    .map(|(previous, current, _, _)| format!("{previous}->{current}"))
                    .collect::<Vec<_>>()
                    .join(",");
                eprintln!("PINPOINT PAGE CURL prewarmed pairs={directions} textures=ready");
            }
        } else if retry {
            self.schedule_curl_prewarm();
        }
    }

    pub fn toggle_blank(&self) {
        self.set_blank(!self.is_blank());
    }

    pub fn set_blank(&self, blank: bool) {
        let blank = {
            let mut state = self.imp().state.borrow_mut();
            state.blank = blank;
            if state.blank {
                let mut media_error = None;
                for media in state.media.values() {
                    if let Err(error) = media.set_playing(false) {
                        media_error = Some(error);
                    }
                }
                if let Some(error) = media_error {
                    state.media_error = Some(error);
                }
                state.active_media = None;
            }
            state.blank
        };
        if !blank {
            self.prepare_working_set();
        }
        self.update_accessibility();
        self.sync_media_offload();
        self.prepare_camera();
        self.queue_draw();
    }

    fn update_accessibility(&self) {
        let text = {
            let state = self.imp().state.borrow();
            accessibility_text(
                state.presentation.as_deref(),
                state.current,
                state.blank,
                &state.accessible_context,
                camera_portal_state(
                    state.camera_media.is_some(),
                    state.camera_request.is_some(),
                    state.camera_request_attempted,
                    state.camera_error.is_some(),
                ),
            )
        };
        self.update_property(&[
            gtk::accessible::Property::Label(&text.label),
            gtk::accessible::Property::Description(&text.description),
        ]);
    }

    fn prepare_working_set(&self) {
        let (presentation, current, path, asset_access, assets, blank) = {
            let state = self.imp().state.borrow();
            let Some(presentation) = state.presentation.clone() else {
                return;
            };
            (
                presentation,
                state.current,
                state.path.clone(),
                state.asset_access,
                state.assets.clone(),
                state.blank,
            )
        };

        let order = (current..presentation.slides.len()).chain((0..current).rev());
        let mut scheduled = 0;
        for index in order {
            let Some(slide) = presentation.slides.get(index) else {
                continue;
            };
            if slide.background_type != BackgroundType::Image {
                continue;
            }
            let Some(background) = slide.background.as_deref() else {
                continue;
            };
            let Some(asset) = resolve_asset(path.as_deref(), background, asset_access) else {
                continue;
            };
            scheduled += 1;
            let weak = self.downgrade();
            assets.prefetch_texture(asset.clone(), move |result| {
                if let Some(stage) = weak.upgrade() {
                    let _ = result;
                    stage.queue_draw();
                }
            });
            if scheduled >= assets.prefetch_slots() {
                break;
            }
        }

        let (suppress_media, media_enabled, audio_enabled, media_looping) = {
            let state = self.imp().state.borrow();
            (
                state.suppress_media,
                state.media_enabled,
                state.audio_enabled,
                state.media_looping,
            )
        };
        let media_paths: Vec<PathBuf> = if suppress_media || !media_enabled {
            Vec::new()
        } else {
            (current..presentation.slides.len())
                .chain((0..current).rev())
                .filter_map(|index| presentation.slides.get(index))
                .filter(|slide| slide.background_type == BackgroundType::Video)
                .filter_map(|slide| slide.background.as_deref())
                .filter_map(|background| resolve_asset(path.as_deref(), background, asset_access))
                .fold(Vec::new(), |mut paths, path| {
                    if !paths.contains(&path) {
                        paths.push(path);
                    }
                    paths
                })
                .into_iter()
                .take(assets.media_prefetch_slots())
                .collect()
        };
        {
            let mut state = self.imp().state.borrow_mut();
            state.media.retain(|path, _| media_paths.contains(path));
            if state
                .active_media
                .as_ref()
                .is_some_and(|path| !state.media.contains_key(path))
            {
                state.active_media = None;
            }
        }
        for media_path in &media_paths {
            if self.imp().state.borrow().media.contains_key(media_path) {
                continue;
            }
            match Media::new_prepared(media_path, audio_enabled, media_looping) {
                Ok(media) => {
                    let media = Rc::new(media);
                    let weak = self.downgrade();
                    media.paintable().connect_invalidate_contents(move |_| {
                        if let Some(stage) = weak.upgrade() {
                            stage.queue_draw();
                        }
                    });
                    let weak = self.downgrade();
                    media.paintable().connect_invalidate_size(move |_| {
                        if let Some(stage) = weak.upgrade() {
                            stage.queue_draw();
                        }
                    });
                    eprintln!("PINPOINT MEDIA prepared {}", media.path().display());
                    self.imp()
                        .state
                        .borrow_mut()
                        .media
                        .insert(media_path.clone(), media);
                }
                Err(error) => {
                    eprintln!("PINPOINT MEDIA failed {}: {error}", media_path.display());
                    self.imp().state.borrow_mut().media_error = Some(error);
                }
            }
        }

        let desired = (!suppress_media).then_some(()).and_then(|()| {
            presentation
                .slides
                .get(current)
                .filter(|slide| slide.background_type == BackgroundType::Video)
                .and_then(|slide| slide.background.as_deref())
                .and_then(|background| resolve_asset(path.as_deref(), background, asset_access))
        });
        let mut state = self.imp().state.borrow_mut();
        let active = if blank {
            None
        } else {
            desired.filter(|path| state.media.contains_key(path))
        };
        let mut media_error = None;
        let mapped = self.is_mapped();
        for (path, media) in &state.media {
            if let Err(error) = media.set_playing(mapped && active.as_ref() == Some(path)) {
                media_error = Some(error);
            }
        }
        if let Some(error) = media_error {
            state.media_error = Some(error);
        }
        if state.active_media != active {
            if let Some(path) = active.as_deref() {
                eprintln!("PINPOINT MEDIA playing {}", path.display());
            }
            state.active_media = active;
        }
        drop(state);
        self.sync_media_offload();
        self.prepare_camera();
    }

    fn sync_media_offload(&self) {
        let mut state = self.imp().state.borrow_mut();
        let eligible = !state.blank
            && state.transition.is_none()
            && state
                .presentation
                .as_ref()
                .and_then(|presentation| presentation.slides.get(state.current))
                .is_some_and(|slide| slide.background_type == BackgroundType::Video);
        let desired = eligible.then(|| state.active_media.clone()).flatten();
        let paintable = desired
            .as_ref()
            .and_then(|path| state.media.get(path))
            .map(|media| media.paintable().clone());
        if let (Some(picture), Some(offload)) =
            (state.media_picture.as_ref(), state.media_offload.as_ref())
        {
            picture.set_paintable(paintable.as_ref());
            offload.set_visible(paintable.is_some());
        }
        if state.offloaded_media != desired {
            state.offloaded_media = desired;
            drop(state);
            self.queue_allocate();
        }
    }

    fn update_active_playback(&self, mapped: bool) {
        let mut state = self.imp().state.borrow_mut();
        let active = state.active_media.clone();
        let mut media_error = None;
        for (path, media) in &state.media {
            if let Err(error) = media.set_playing(mapped && active.as_ref() == Some(path)) {
                media_error = Some(error);
            }
        }
        if let Some(error) = media_error {
            state.media_error = Some(error);
        }
    }

    fn prepare_camera(&self) {
        let on_camera = {
            let state = self.imp().state.borrow();
            state.media_enabled
                && state.camera_enabled
                && !state.suppress_media
                && !state.blank
                && state
                    .presentation
                    .as_ref()
                    .and_then(|presentation| presentation.slides.get(state.current))
                    .is_some_and(|slide| slide.background_type == BackgroundType::Camera)
        };
        let active_window = self
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok())
            .is_some_and(|window| window.is_active());
        {
            let mut state = self.imp().state.borrow_mut();
            if let Some(media) = state.camera_media.as_ref()
                && let Err(error) = media.set_playing(on_camera && active_window)
            {
                state.camera_error = Some(error);
            }
            if !on_camera {
                state.camera_request = None;
                state.camera_request_attempted = false;
                return;
            }
            if !active_window {
                if !state.camera_retry_pending {
                    state.camera_retry_pending = true;
                    let weak = self.downgrade();
                    glib::timeout_add_local_once(Duration::from_millis(100), move || {
                        if let Some(stage) = weak.upgrade() {
                            stage.imp().state.borrow_mut().camera_retry_pending = false;
                            stage.prepare_camera();
                        }
                    });
                }
                return;
            }
            if state.camera_media.is_some()
                || state.camera_request.is_some()
                || state.camera_request_attempted
            {
                return;
            }
            state.camera_request_attempted = true;
            state.camera_retry_pending = false;
        }

        let weak = self.downgrade();
        let request = CameraRequest::start(move |event| {
            let Some(stage) = weak.upgrade() else {
                return;
            };
            let mut state = stage.imp().state.borrow_mut();
            state.camera_request = None;
            match event {
                CameraPortalEvent::Ready(media) => {
                    let media = Rc::new(media);
                    let weak = stage.downgrade();
                    media.paintable().connect_invalidate_contents(move |_| {
                        if let Some(stage) = weak.upgrade() {
                            stage.queue_draw();
                        }
                    });
                    let weak = stage.downgrade();
                    media.paintable().connect_invalidate_size(move |_| {
                        if let Some(stage) = weak.upgrade() {
                            stage.queue_allocate();
                            stage.queue_draw();
                        }
                    });
                    if let Err(error) = media.set_playing(true) {
                        state.camera_error = Some(error);
                    } else {
                        eprintln!("PINPOINT CAMERA portal-ready pipewire=true");
                        state.camera_media = Some(media);
                    }
                }
                CameraPortalEvent::Denied => {
                    eprintln!("PINPOINT CAMERA portal-denied");
                }
                CameraPortalEvent::Error(error) => {
                    eprintln!("PINPOINT CAMERA failed: {error}");
                    state.camera_error = Some(error);
                }
            }
            drop(state);
            stage.update_accessibility();
            stage.queue_draw();
        });
        self.imp().state.borrow_mut().camera_request = Some(request);
        eprintln!("PINPOINT CAMERA portal-requested");
    }
}

fn allocate_media_offload(stage: &Stage, state: &mut State, width: f32, height: f32) {
    let (Some(offload), Some(path), Some(presentation)) = (
        state.media_offload.as_ref(),
        state.offloaded_media.as_ref(),
        state.presentation.as_ref(),
    ) else {
        state.allocated_offloaded_media = None;
        return;
    };
    let Some(slide) = presentation.slides.get(state.current) else {
        state.allocated_offloaded_media = None;
        return;
    };
    let Some(media) = state.media.get(path) else {
        state.allocated_offloaded_media = None;
        return;
    };
    let paintable = media.paintable();
    let intrinsic_width = paintable.intrinsic_width().max(1) as f32;
    let intrinsic_height = paintable.intrinsic_height().max(1) as f32;
    let mut rect = background_rect(slide, width, height, intrinsic_width, intrinsic_height);
    let scale = stage
        .native()
        .and_then(|native| native.surface())
        .map_or(stage.scale_factor() as f32, |surface| {
            surface.scale() as f32
        })
        .max(1.0);
    rect.x = (rect.x * scale).round() / scale;
    rect.y = (rect.y * scale).round() / scale;
    rect.width = (rect.width * scale).round() / scale;
    rect.height = (rect.height * scale).round() / scale;
    let transform = gsk::Transform::new().translate(&graphene::Point::new(rect.x, rect.y));
    offload.allocate(
        rect.width.max(1.0).round() as i32,
        rect.height.max(1.0).round() as i32,
        -1,
        Some(transform),
    );
    state.allocated_offloaded_media = Some(path.clone());
}

fn media_offload_has_current_allocation(
    offloaded_media: Option<&PathBuf>,
    allocated_offloaded_media: Option<&PathBuf>,
) -> bool {
    offloaded_media.is_some() && offloaded_media == allocated_offloaded_media
}

fn resolve_asset(
    presentation_path: Option<&Path>,
    asset: &str,
    access: pinpoint_core::asset::Access,
) -> Option<PathBuf> {
    pinpoint_core::asset::resolve_local(presentation_path, asset, access)
}

fn legacy_transition(state: &mut State, slide: &Slide) -> Option<Rc<LegacyTransition>> {
    if slide.transition.is_empty() || is_builtin(&slide.transition) {
        return None;
    }
    if let Some(cached) = state.legacy_transitions.get(&slide.transition) {
        return cached.clone();
    }
    let filename = if slide.transition.ends_with(".json") {
        slide.transition.clone()
    } else {
        format!("{}.json", slide.transition)
    };
    let loaded = resolve_asset(state.path.as_deref(), &filename, state.asset_access).and_then(
        |path| match LegacyTransition::load(&path) {
            Ok(transition) => Some(Rc::new(transition)),
            Err(error) => {
                eprintln!(
                    "PINPOINT TRANSITION unable to load legacy transition {}: {error}; using fade",
                    path.display()
                );
                None
            }
        },
    );
    state
        .legacy_transitions
        .insert(slide.transition.clone(), loaded.clone());
    loaded
}

fn parse_color(value: &str, fallback: &str) -> gdk::RGBA {
    gdk::RGBA::parse(value)
        .or_else(|_| gdk::RGBA::parse(fallback))
        .expect("fallback GDK colour is valid")
}

fn graphene_rect(rect: Rect) -> graphene::Rect {
    graphene::Rect::new(rect.x, rect.y, rect.width, rect.height)
}

fn snapshot_layer_begin(snapshot: &gtk::Snapshot, layer: LayerState, width: f32, height: f32) {
    snapshot.save();
    snapshot.translate(&graphene::Point::new(layer.x, layer.y));
    snapshot.translate(&graphene::Point::new(width / 2.0, height / 2.0));
    if layer.angle_x != 0.0 || layer.angle_y != 0.0 {
        snapshot.perspective(width.max(height) * 2.0);
    }
    if layer.angle_x != 0.0 {
        snapshot.rotate_3d(layer.angle_x, &graphene::Vec3::new(1.0, 0.0, 0.0));
    }
    if layer.angle_y != 0.0 {
        snapshot.rotate_3d(layer.angle_y, &graphene::Vec3::new(0.0, 1.0, 0.0));
    }
    snapshot.rotate(layer.angle);
    snapshot.scale(layer.scale_x, layer.scale_y);
    snapshot.translate(&graphene::Point::new(-width / 2.0, -height / 2.0));
    if layer.opacity < 1.0 {
        snapshot.push_opacity(layer.opacity.clamp(0.0, 1.0).into());
    }
}

fn snapshot_layer_end(snapshot: &gtk::Snapshot, layer: LayerState) {
    if layer.opacity < 1.0 {
        snapshot.pop();
    }
    snapshot.restore();
}

fn snapshot_stage(
    widget: &Stage,
    snapshot: &gtk::Snapshot,
    state: &mut State,
    width: f32,
    height: f32,
) {
    let bounds = graphene::Rect::new(0.0, 0.0, width, height);
    if state.blank || state.presentation.is_none() || width <= 0.0 || height <= 0.0 {
        snapshot.append_color(&parse_color("black", "black"), &bounds);
        return;
    }
    let presentation = state.presentation.as_ref().unwrap().clone();
    if let Some(transition) = state.transition {
        let duration = transition.duration.as_secs_f64();
        let progress = if duration <= f64::EPSILON {
            1.0
        } else {
            (transition.started.elapsed().as_secs_f64() / duration).clamp(0.0, 1.0) as f32
        };
        let previous_slide = &presentation.slides[transition.previous];
        let current_slide = &presentation.slides[state.current];
        let previous_legacy = legacy_transition(state, previous_slide);
        let current_legacy = legacy_transition(state, current_slide);
        match transition_renderer::plan_with_legacy(
            previous_slide,
            current_slide,
            transition.backwards,
            progress.into(),
            previous_legacy.as_deref(),
            current_legacy.as_deref(),
        ) {
            TransitionPlan::Layers(plan) => {
                let current_index = state.current;
                let mut draw = |index, active, transition| {
                    snapshot_slide(
                        widget,
                        snapshot,
                        state,
                        &presentation,
                        index,
                        active,
                        transition,
                        width,
                        height,
                    );
                };
                if plan.previous_above {
                    draw(current_index, true, plan.current);
                    draw(transition.previous, false, plan.previous);
                } else {
                    draw(transition.previous, false, plan.previous);
                    draw(current_index, true, plan.current);
                }
            }
            TransitionPlan::PageCurl(plan) => {
                if let Some((previous, current)) = page_curl_slide_textures(
                    widget,
                    state,
                    &presentation,
                    transition,
                    state.current,
                    width,
                    height,
                ) {
                    if let Some(curl_view) = state.curl_view.as_ref() {
                        curl_view.set_transition(
                            &previous,
                            &current,
                            plan.previous.period,
                            plan.previous.angle,
                            plan.current.period,
                            plan.current.angle,
                            plan.backwards,
                        );
                        widget.snapshot_child(curl_view.widget(), snapshot);
                    }
                    let rendered = Instant::now();
                    if state.curl_timing_generation == transition.generation {
                        let elapsed = state
                            .curl_last_frame
                            .map_or(Duration::ZERO, |last| rendered.duration_since(last));
                        state.page_curl_worst_ms =
                            state.page_curl_worst_ms.max(elapsed.as_secs_f64() * 1000.0);
                        state
                            .page_curl_snapshot_intervals_ms
                            .push(elapsed.as_secs_f64() * 1000.0);
                    } else {
                        state.curl_timing_generation = transition.generation;
                        let setup = rendered.duration_since(transition.started);
                        state.page_curl_setup_worst_ms = state
                            .page_curl_setup_worst_ms
                            .max(setup.as_secs_f64() * 1000.0);
                    }
                    state.curl_last_frame = Some(rendered);
                    state.page_curl_frames = state.page_curl_frames.saturating_add(1);
                }
            }
        }
    } else {
        snapshot_slide(
            widget,
            snapshot,
            state,
            &presentation,
            state.current,
            true,
            TransitionState::default(),
            width,
            height,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn snapshot_slide_to_node(
    widget: &Stage,
    state: &mut State,
    presentation: &Presentation,
    index: usize,
    active: bool,
    width: f32,
    height: f32,
    output_scale: i32,
) -> Option<gsk::RenderNode> {
    let snapshot = gtk::Snapshot::new();
    if output_scale > 1 {
        snapshot.scale(output_scale as f32, output_scale as f32);
    }
    snapshot_slide(
        widget,
        &snapshot,
        state,
        presentation,
        index,
        active,
        TransitionState::default(),
        width,
        height,
    );
    snapshot.to_node()
}

fn page_curl_slide_textures(
    widget: &Stage,
    state: &mut State,
    presentation: &Presentation,
    transition: Transition,
    current_index: usize,
    width: f32,
    height: f32,
) -> Option<(gdk::Texture, gdk::Texture)> {
    let scale = widget.scale_factor();
    let output = OutputKey {
        width: width as i32,
        height: height as i32,
        scale,
    };
    if let Some(pair) = curl_cache_pair(
        &state.curl_cache,
        transition.previous,
        current_index,
        output,
    ) {
        return Some(pair);
    }

    let renderer = widget.native()?.renderer()?;
    let viewport = graphene::Rect::new(0.0, 0.0, width * scale as f32, height * scale as f32);
    let previous = snapshot_slide_to_node(
        widget,
        state,
        presentation,
        transition.previous,
        false,
        width,
        height,
        scale,
    )?;
    let current = snapshot_slide_to_node(
        widget,
        state,
        presentation,
        current_index,
        true,
        width,
        height,
        scale,
    )?;
    let previous_texture = renderer.render_texture(&previous, Some(&viewport));
    let current_texture = renderer.render_texture(&current, Some(&viewport));
    let capacity = curl_cache_capacity(output);
    if capacity > 0 {
        state
            .curl_cache
            .retain(|cache| cache.width == width && cache.height == height && cache.scale == scale);
        if state.curl_cache.len() >= capacity {
            state.curl_cache.remove(0);
        }
        state.curl_cache.push(CurlCache {
            previous_index: transition.previous,
            current_index,
            width,
            height,
            scale,
            previous: previous_texture.clone(),
            current: current_texture.clone(),
        });
    }
    Some((previous_texture, current_texture))
}

fn curl_cache_capacity(output: OutputKey) -> usize {
    let width = output.width.max(0) as usize;
    let height = output.height.max(0) as usize;
    let scale = output.scale.max(1) as usize;
    let Some(bytes_per_pair) = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(scale))
        .and_then(|pixels| pixels.checked_mul(scale))
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes_per_slide| bytes_per_slide.checked_mul(2))
    else {
        return 0;
    };
    if bytes_per_pair == 0 || bytes_per_pair > CURL_PREWARM_BUDGET_BYTES {
        0
    } else if bytes_per_pair <= CURL_PREWARM_BUDGET_BYTES / 2 {
        2
    } else {
        1
    }
}

fn curl_cache_pair(
    cache: &[CurlCache],
    previous: usize,
    current: usize,
    output: OutputKey,
) -> Option<(gdk::Texture, gdk::Texture)> {
    cache.iter().find_map(|cache| {
        (cache.width == output.width as f32
            && cache.height == output.height as f32
            && cache.scale == output.scale)
            .then(|| {
                if cache.previous_index == previous && cache.current_index == current {
                    Some((cache.previous.clone(), cache.current.clone()))
                } else if cache.previous_index == current && cache.current_index == previous {
                    Some((cache.current.clone(), cache.previous.clone()))
                } else {
                    None
                }
            })
            .flatten()
    })
}

fn curl_pair_is_static(
    presentation: &Presentation,
    previous: usize,
    current: usize,
    backwards: bool,
) -> bool {
    let Some(previous_slide) = presentation.slides.get(previous) else {
        return false;
    };
    let Some(current_slide) = presentation.slides.get(current) else {
        return false;
    };
    matches!(
        transition_renderer::plan(previous_slide, current_slide, backwards, 0.0),
        TransitionPlan::PageCurl(_)
    ) && !matches!(
        previous_slide.background_type,
        BackgroundType::Video | BackgroundType::Camera
    ) && !matches!(
        current_slide.background_type,
        BackgroundType::Video | BackgroundType::Camera
    )
}

#[allow(clippy::too_many_arguments)]
fn snapshot_slide(
    widget: &Stage,
    snapshot: &gtk::Snapshot,
    state: &mut State,
    presentation: &Presentation,
    index: usize,
    active: bool,
    transition: TransitionState,
    width: f32,
    height: f32,
) {
    let Some(slide) = presentation.slides.get(index) else {
        return;
    };
    let bounds = graphene::Rect::new(0.0, 0.0, width, height);
    snapshot_layer_begin(snapshot, transition.actor, width, height);
    snapshot.append_color(&parse_color(&slide.stage_color, "black"), &bounds);
    snapshot_layer_begin(snapshot, transition.background, width, height);
    snapshot_background(widget, snapshot, state, slide, index, active, width, height);
    snapshot_layer_end(snapshot, transition.background);
    if active && slide.background_type == BackgroundType::Camera && state.camera_media.is_none() {
        snapshot_camera_status(widget, snapshot, state, width, height);
    }
    if let Some(nodes) = text_nodes(widget, state, slide, index, width, height) {
        snapshot_layer_begin(snapshot, transition.midground, width, height);
        snapshot.append_node(&nodes.shading);
        snapshot_layer_end(snapshot, transition.midground);
        snapshot_layer_begin(snapshot, transition.foreground, width, height);
        snapshot.append_node(&nodes.foreground);
        snapshot_layer_end(snapshot, transition.foreground);
    }
    snapshot_layer_end(snapshot, transition.actor);
}

fn snapshot_camera_status(
    widget: &Stage,
    snapshot: &gtk::Snapshot,
    state: &State,
    width: f32,
    height: f32,
) {
    let status = camera_portal_state(
        state.camera_media.is_some(),
        state.camera_request.is_some(),
        state.camera_request_attempted,
        state.camera_error.is_some(),
    );
    let Some((heading, detail)) = camera_status_message(status) else {
        return;
    };
    let layout = widget.create_pango_layout(Some(&format!("{heading}\n{detail}")));
    layout.set_font_description(Some(&pango::FontDescription::from_string("Sans 22px")));
    layout.set_alignment(pango::Alignment::Center);
    layout.set_wrap(pango::WrapMode::WordChar);
    layout.set_width((width * 0.72 * pango::SCALE as f32) as i32);
    let (_, logical) = layout.pixel_extents();
    let panel_width = (logical.width() as f32 + 48.0).min(width * 0.82);
    let panel_height = logical.height() as f32 + 44.0;
    let panel = Rect {
        x: (width - panel_width) / 2.0,
        y: (height - panel_height) / 2.0,
        width: panel_width,
        height: panel_height,
    };
    let mut panel_color = parse_color("black", "black");
    panel_color.set_alpha(0.72);
    snapshot.append_color(&panel_color, &graphene_rect(panel));
    snapshot.save();
    snapshot.translate(&graphene::Point::new(panel.x + 24.0, panel.y + 22.0));
    snapshot.append_layout(&layout, &parse_color("white", "white"));
    snapshot.restore();
}

#[allow(clippy::too_many_arguments)]
fn snapshot_background(
    widget: &Stage,
    snapshot: &gtk::Snapshot,
    state: &mut State,
    slide: &Slide,
    index: usize,
    active: bool,
    width: f32,
    height: f32,
) {
    let bounds = graphene::Rect::new(0.0, 0.0, width, height);
    let Some(background) = slide.background.as_deref() else {
        return;
    };
    match slide.background_type {
        BackgroundType::Color => {
            snapshot.append_color(&parse_color(background, "black"), &bounds);
        }
        BackgroundType::Image => {
            let Some(path) = resolve_asset(state.path.as_deref(), background, state.asset_access)
            else {
                return;
            };
            let Some(texture) = state.assets.texture(&path) else {
                return;
            };
            let rect = background_rect(
                slide,
                width,
                height,
                texture.width() as f32,
                texture.height() as f32,
            );
            snapshot.push_clip(&bounds);
            snapshot.append_scaled_texture(
                &texture,
                if rect.width < texture.width() as f32 || rect.height < texture.height() as f32 {
                    gsk::ScalingFilter::Trilinear
                } else {
                    gsk::ScalingFilter::Linear
                },
                &graphene_rect(rect),
            );
            snapshot.pop();
        }
        BackgroundType::Svg => {
            if let Some(node) = svg_node(state, slide, index, width, height) {
                snapshot.append_node(node);
            }
        }
        BackgroundType::Video if active => {
            if media_offload_has_current_allocation(
                state.offloaded_media.as_ref(),
                state.allocated_offloaded_media.as_ref(),
            ) && let Some(offload) = state.media_offload.as_ref()
            {
                widget.snapshot_child(offload, snapshot);
                return;
            }
            let Some(path) = state.active_media.as_ref() else {
                return;
            };
            let Some(media) = state.media.get(path) else {
                return;
            };
            let paintable = media.paintable();
            let intrinsic_width = paintable.intrinsic_width().max(1) as f32;
            let intrinsic_height = paintable.intrinsic_height().max(1) as f32;
            let rect = background_rect(slide, width, height, intrinsic_width, intrinsic_height);
            snapshot.push_clip(&bounds);
            snapshot.save();
            snapshot.translate(&graphene::Point::new(rect.x, rect.y));
            paintable.snapshot(snapshot, rect.width.into(), rect.height.into());
            snapshot.restore();
            snapshot.pop();
        }
        BackgroundType::Camera if active => {
            let Some(media) = state.camera_media.as_ref() else {
                return;
            };
            let paintable = media.paintable();
            let intrinsic_width = paintable.intrinsic_width().max(1) as f32;
            let intrinsic_height = paintable.intrinsic_height().max(1) as f32;
            let rect = background_rect(slide, width, height, intrinsic_width, intrinsic_height);
            snapshot.push_clip(&bounds);
            snapshot.save();
            snapshot.translate(&graphene::Point::new(rect.x, rect.y));
            paintable.snapshot(snapshot, rect.width.into(), rect.height.into());
            snapshot.restore();
            snapshot.pop();
        }
        BackgroundType::Video | BackgroundType::Camera | BackgroundType::None => {}
    }
}

fn svg_node(
    state: &mut State,
    slide: &Slide,
    index: usize,
    width: f32,
    height: f32,
) -> Option<gsk::RenderNode> {
    let key = (index, width.round() as i32, height.round() as i32);
    if let Some(node) = state.svg_nodes.get(&key) {
        return Some(node.clone());
    }
    let background = slide.background.as_deref()?;
    let path = resolve_asset(state.path.as_deref(), background, state.asset_access)?;
    let handle = match state.assets.svg(&path) {
        Ok(handle) => handle,
        Err(_) => return None,
    };
    let renderer = rsvg::CairoRenderer::new(&handle);
    let (intrinsic_width, intrinsic_height) = renderer
        .intrinsic_size_in_pixels()
        .unwrap_or((width.into(), height.into()));
    let rect = background_rect(
        slide,
        width,
        height,
        intrinsic_width as f32,
        intrinsic_height as f32,
    );
    let svg_snapshot = gtk::Snapshot::new();
    let bounds = graphene::Rect::new(0.0, 0.0, width, height);
    let context = svg_snapshot.append_cairo(&bounds);
    let viewport = cairo::Rectangle::new(
        rect.x.into(),
        rect.y.into(),
        rect.width.into(),
        rect.height.into(),
    );
    if let Err(error) = renderer.render_document(&context, &viewport) {
        eprintln!(
            "PINPOINT ASSET svg-render-failed {}: {error}",
            path.display()
        );
        return None;
    }
    if let Some(node) = svg_snapshot.to_node() {
        state.svg_nodes.insert(key, node);
    }
    state.svg_nodes.get(&key).cloned()
}

fn add_asset_problem(
    problems: &mut Vec<(PathBuf, String, Vec<usize>)>,
    path: PathBuf,
    error: String,
    slide: usize,
) {
    if let Some((_, _, slides)) = problems
        .iter_mut()
        .find(|(candidate, _, _)| *candidate == path)
    {
        if !slides.contains(&slide) {
            slides.push(slide);
        }
    } else {
        problems.push((path, error, vec![slide]));
    }
}

fn format_slide_numbers(slides: &[usize]) -> String {
    let values = slides.iter().map(usize::to_string).collect::<Vec<_>>();
    match values.as_slice() {
        [] => String::new(),
        [only] => format!(" {only}"),
        [first, second] => format!("s {first} and {second}"),
        _ => format!(
            "s {}, and {}",
            values[..values.len() - 1].join(", "),
            values.last().expect("non-empty slide list")
        ),
    }
}

fn problem_paths(state: &State) -> Vec<PathBuf> {
    let mut paths = state.assets.failed_paths();
    if let Some(presentation) = state.presentation.as_ref() {
        for slide in &presentation.slides {
            if matches!(
                slide.background_type,
                BackgroundType::Image | BackgroundType::Svg | BackgroundType::Video
            ) && let Some(background) = slide.background.as_deref()
                && let Some(path) =
                    resolve_asset(state.path.as_deref(), background, state.asset_access)
                && (!path.exists()
                    || state
                        .media
                        .get(&path)
                        .is_some_and(|media| media.error().is_some()))
                && !paths.contains(&path)
            {
                paths.push(path);
            }
        }
    }
    paths
}

fn text_nodes(
    widget: &Stage,
    state: &mut State,
    slide: &Slide,
    index: usize,
    width: f32,
    height: f32,
) -> Option<TextNodes> {
    let key = (index, width.round() as i32, height.round() as i32);
    if let Some(node) = state.text_nodes.get(&key) {
        return Some(node.clone());
    }
    let text = slide.text.as_deref()?;
    if text.is_empty() {
        return None;
    }
    let layout = widget.create_pango_layout(None);
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
    let (text, scale) = text_rect(
        slide,
        width,
        height,
        logical.width() as f32,
        logical.height() as f32,
    );
    let shade = shading_rect(width, text);
    let mut shading_color = parse_color(&slide.shading_color, "black");
    shading_color.set_alpha(
        (shading_color.alpha() * slide.shading_opacity.clamp(0.0, 1.0) as f32).clamp(0.0, 1.0),
    );
    let shading_snapshot = gtk::Snapshot::new();
    shading_snapshot.append_color(&shading_color, &graphene_rect(shade));
    let foreground_snapshot = gtk::Snapshot::new();
    foreground_snapshot.save();
    foreground_snapshot.translate(&graphene::Point::new(text.x, text.y));
    foreground_snapshot.scale(scale, scale);
    foreground_snapshot.append_layout(&layout, &parse_color(&slide.text_color, "white"));
    foreground_snapshot.restore();
    let (Some(shading), Some(foreground)) =
        (shading_snapshot.to_node(), foreground_snapshot.to_node())
    else {
        return None;
    };
    state.text_nodes.insert(
        key,
        TextNodes {
            shading,
            foreground,
        },
    );
    state.text_nodes.get(&key).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presentation(slides: Vec<Slide>) -> Presentation {
        Presentation {
            defaults: Slide::default(),
            slides,
            source: String::new(),
            asset_access: pinpoint_core::asset::Access::Confined,
        }
    }

    #[test]
    fn curl_prewarm_only_caches_static_pairs() {
        let mut outgoing = Slide {
            transition: "page-curl".into(),
            ..Slide::default()
        };
        let incoming = Slide::default();
        let deck = presentation(vec![outgoing.clone(), incoming.clone()]);
        assert!(curl_pair_is_static(&deck, 0, 1, false));

        outgoing.background_type = BackgroundType::Video;
        let deck = presentation(vec![outgoing, incoming]);
        assert!(!curl_pair_is_static(&deck, 0, 1, false));
    }

    #[test]
    fn curl_cache_budget_keeps_two_common_hidpi_pairs() {
        assert_eq!(
            curl_cache_capacity(OutputKey {
                width: 1920,
                height: 1080,
                scale: 2,
            }),
            2
        );
        assert_eq!(
            curl_cache_capacity(OutputKey {
                width: 3840,
                height: 2160,
                scale: 2,
            }),
            1
        );
    }

    #[test]
    fn asset_problem_slide_lists_are_concise() {
        assert_eq!(format_slide_numbers(&[2]), " 2");
        assert_eq!(format_slide_numbers(&[2, 5]), "s 2 and 5");
        assert_eq!(format_slide_numbers(&[2, 5, 9]), "s 2, 5, and 9");

        let mut problems = Vec::new();
        add_asset_problem(
            &mut problems,
            PathBuf::from("missing.png"),
            "missing".into(),
            2,
        );
        add_asset_problem(
            &mut problems,
            PathBuf::from("missing.png"),
            "missing".into(),
            5,
        );
        assert_eq!(problems[0].2, vec![2, 5]);
    }

    #[test]
    fn camera_statuses_are_actionable_without_a_device_path() {
        assert_eq!(
            camera_status_message(CameraPortalState::Pending),
            Some((
                "Camera permission requested",
                "Approve the desktop camera request to continue.",
            ))
        );
        assert_eq!(
            camera_status_message(CameraPortalState::Denied),
            Some((
                "Camera access was denied",
                "Press C to request access again.",
            ))
        );
        assert_eq!(camera_status_message(CameraPortalState::Ready), None);
    }

    #[test]
    fn offload_snapshot_waits_for_matching_allocation() {
        let video = PathBuf::from("video.webm");
        let previous_video = PathBuf::from("previous-video.webm");

        assert!(!media_offload_has_current_allocation(Some(&video), None));
        assert!(!media_offload_has_current_allocation(
            Some(&video),
            Some(&previous_video)
        ));
        assert!(media_offload_has_current_allocation(
            Some(&video),
            Some(&video)
        ));
        assert!(!media_offload_has_current_allocation(None, None));
    }

    #[test]
    fn accessibility_text_matches_the_stage_contract() {
        let slide = Slide {
            text: Some("<b>Visible audience text</b>".into()),
            visual_description: Some("A chart with an upward trend".into()),
            ..Slide::default()
        };
        let presentation = Presentation {
            defaults: Slide::default(),
            slides: vec![slide],
            source: String::new(),
            asset_access: pinpoint_core::asset::Access::Confined,
        };
        assert_eq!(
            accessibility_text(
                Some(&presentation),
                0,
                false,
                "Editor preview",
                CameraPortalState::Ready,
            ),
            AccessibilityText {
                label: "Editor preview 1 of 1".into(),
                description:
                    "Visible audience text\nVisual description: A chart with an upward trend".into(),
            }
        );
        assert_eq!(
            accessibility_text(
                Some(&presentation),
                0,
                true,
                "Presentation slide",
                CameraPortalState::Ready,
            )
            .description,
            "Blank screen"
        );
    }

    #[test]
    fn reduced_motion_completes_transitions_immediately() {
        let duration = Duration::from_millis(800);
        assert_eq!(
            motion_duration(duration, false, gtk::ReducedMotion::NoPreference),
            Duration::ZERO
        );
        assert_eq!(
            motion_duration(duration, true, gtk::ReducedMotion::Reduce),
            Duration::ZERO
        );
        assert_eq!(
            motion_duration(duration, true, gtk::ReducedMotion::NoPreference),
            duration
        );
    }
}
