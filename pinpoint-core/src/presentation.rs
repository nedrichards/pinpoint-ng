use crate::asset;
use serde::Serialize;
use std::fmt;
use std::io::Read;
use std::path::Path;

pub const MAX_PRESENTATION_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_SLIDES: usize = 1_024;
pub const MAX_SETTING_BYTES: usize = 64 * 1024;
pub const MAX_SLIDE_TEXT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_NOTES_BYTES: usize = 1024 * 1024;
pub const MAX_VISUAL_DESCRIPTION_BYTES: usize = 256 * 1024;
pub const MAX_MARKUP_DEPTH: usize = 128;
const MAX_DURATION_SECONDS: f64 = 24.0 * 60.0 * 60.0;
const MAX_TRANSITION_DURATION_MS: u32 = 60_000;
const MAX_CAMERA_DIMENSION: i32 = 16_384;
const MAX_CAMERA_FRAMERATE: i32 = 240;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Gravity {
    Center,
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum BackgroundType {
    None,
    Color,
    Image,
    Video,
    Camera,
    Svg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum BackgroundScale {
    Unscaled,
    Fit,
    Fill,
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum TransitionDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum TransitionLayer {
    Default,
    All,
    Background,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum TransitionMode {
    Both,
    In,
    Out,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Resolution {
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Slide {
    pub stage_color: String,
    pub background: Option<String>,
    pub background_type: BackgroundType,
    pub background_scale: BackgroundScale,
    pub background_position: Gravity,
    pub text: Option<String>,
    pub text_position: Gravity,
    pub font: String,
    pub notes_font: String,
    pub notes_font_size: String,
    pub text_align: TextAlign,
    pub text_color: String,
    pub use_markup: bool,
    pub duration: f64,
    pub new_duration: f64,
    pub speaker_notes: Option<String>,
    pub visual_description: Option<String>,
    pub shading_color: String,
    pub shading_opacity: f64,
    pub transition: String,
    pub transition_direction: TransitionDirection,
    pub transition_layer: TransitionLayer,
    pub transition_mode: TransitionMode,
    pub transition_duration_ms: u32,
    pub transition_easing: String,
    pub command: Option<String>,
    pub camera_framerate: i32,
    pub camera_resolution: Resolution,
}

impl Default for Slide {
    fn default() -> Self {
        Self {
            stage_color: "black".into(),
            background: None,
            background_type: BackgroundType::None,
            background_scale: BackgroundScale::Fit,
            background_position: Gravity::Center,
            text: None,
            text_position: Gravity::Center,
            font: "Sans 60px".into(),
            notes_font: "Sans".into(),
            notes_font_size: "20px".into(),
            text_align: TextAlign::Left,
            text_color: "white".into(),
            use_markup: true,
            duration: 30.0,
            new_duration: 0.0,
            speaker_notes: None,
            visual_description: None,
            shading_color: "black".into(),
            shading_opacity: 0.66,
            transition: "fade".into(),
            transition_direction: TransitionDirection::Left,
            transition_layer: TransitionLayer::Default,
            transition_mode: TransitionMode::Both,
            transition_duration_ms: 0,
            transition_easing: "linear".into(),
            command: None,
            camera_framerate: 0,
            camera_resolution: Resolution::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Presentation {
    pub defaults: Slide,
    pub slides: Vec<Slide>,
    #[serde(skip)]
    pub source: String,
    #[serde(skip)]
    pub asset_access: asset::Access,
}

#[derive(Debug)]
pub enum ParseError {
    Io(std::io::Error),
    InvalidUtf8(std::str::Utf8Error),
    NulByte,
    TooLarge,
    TooManySlides,
    SettingTooLong,
    SlideTextTooLong,
    NotesTooLong,
    VisualDescriptionTooLong,
    MarkupTooDeep,
    NoSlideSeparator,
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Unable to read presentation: {error}"),
            Self::InvalidUtf8(_) => write!(formatter, "The presentation is not valid UTF-8"),
            Self::NulByte => write!(formatter, "The presentation contains a NUL byte"),
            Self::TooLarge => write!(
                formatter,
                "The presentation exceeds the {} MiB safety limit",
                MAX_PRESENTATION_BYTES / 1024 / 1024
            ),
            Self::TooManySlides => write!(
                formatter,
                "The presentation exceeds the {MAX_SLIDES} slide safety limit"
            ),
            Self::SettingTooLong => write!(
                formatter,
                "A presentation setting exceeds the {} KiB safety limit",
                MAX_SETTING_BYTES / 1024
            ),
            Self::SlideTextTooLong => write!(
                formatter,
                "A slide exceeds the {} MiB text safety limit",
                MAX_SLIDE_TEXT_BYTES / 1024 / 1024
            ),
            Self::NotesTooLong => write!(
                formatter,
                "A slide exceeds the {} MiB speaker-note safety limit",
                MAX_NOTES_BYTES / 1024 / 1024
            ),
            Self::VisualDescriptionTooLong => write!(
                formatter,
                "A slide exceeds the {} KiB visual-description safety limit",
                MAX_VISUAL_DESCRIPTION_BYTES / 1024
            ),
            Self::MarkupTooDeep => write!(
                formatter,
                "A slide exceeds the {MAX_MARKUP_DEPTH}-level markup safety limit"
            ),
            Self::NoSlideSeparator => {
                write!(
                    formatter,
                    "The presentation does not contain a slide separator"
                )
            }
        }
    }
}

impl std::error::Error for ParseError {}

fn replace(destination: &mut String, value: &str) {
    destination.clear();
    destination.push_str(value);
}

fn parse_resolution(value: &str) -> Resolution {
    let Some((width, height)) = value.split_once('x') else {
        return Resolution::default();
    };
    match (width.parse(), height.parse()) {
        (Ok(width), Ok(height))
            if (1..=MAX_CAMERA_DIMENSION).contains(&width)
                && (1..=MAX_CAMERA_DIMENSION).contains(&height) =>
        {
            Resolution { width, height }
        }
        _ => Resolution::default(),
    }
}

fn finite_in_range(value: &str, minimum: f64, maximum: f64) -> Option<f64> {
    value
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && (*value >= minimum) && (*value <= maximum))
}

fn parse_setting(slide: &mut Slide, setting: &str) {
    if let Some(value) = setting.strip_prefix("stage-color=") {
        replace(&mut slide.stage_color, value);
    } else if let Some(value) = setting.strip_prefix("font=") {
        replace(&mut slide.font, value);
    } else if let Some(value) = setting.strip_prefix("notes-font=") {
        replace(&mut slide.notes_font, value);
    } else if let Some(value) = setting.strip_prefix("notes-font-size=") {
        replace(&mut slide.notes_font_size, value);
    } else if let Some(value) = setting.strip_prefix("text-color=") {
        replace(&mut slide.text_color, value);
    } else if let Some(value) = setting.strip_prefix("text-align=") {
        slide.text_align = match value {
            "center" => TextAlign::Center,
            "right" => TextAlign::Right,
            _ => TextAlign::Left,
        };
    } else if let Some(value) = setting.strip_prefix("shading-color=") {
        replace(&mut slide.shading_color, value);
    } else if let Some(value) = setting.strip_prefix("shading-opacity=") {
        slide.shading_opacity = finite_in_range(value, 0.0, 1.0).unwrap_or(0.0);
    } else if let Some(value) = setting.strip_prefix("duration=") {
        slide.duration = finite_in_range(value, 0.0, MAX_DURATION_SECONDS).unwrap_or(0.0);
    } else if let Some(value) = setting.strip_prefix("command=") {
        slide.command = Some(value.into());
    } else if let Some(value) = setting.strip_prefix("transition=") {
        replace(&mut slide.transition, value);
    } else if let Some(value) = setting.strip_prefix("transition-direction=") {
        slide.transition_direction = match value {
            "right" => TransitionDirection::Right,
            "up" => TransitionDirection::Up,
            "down" => TransitionDirection::Down,
            _ => TransitionDirection::Left,
        };
    } else if let Some(value) = setting.strip_prefix("transition-layer=") {
        slide.transition_layer = match value {
            "default" => TransitionLayer::Default,
            "background" => TransitionLayer::Background,
            "text" => TransitionLayer::Text,
            _ => TransitionLayer::All,
        };
    } else if let Some(value) = setting.strip_prefix("transition-mode=") {
        slide.transition_mode = match value {
            "in" => TransitionMode::In,
            "out" => TransitionMode::Out,
            _ => TransitionMode::Both,
        };
    } else if let Some(value) = setting.strip_prefix("transition-duration=") {
        slide.transition_duration_ms = value.parse::<u64>().map_or(0, |duration| {
            duration.min(MAX_TRANSITION_DURATION_MS.into()) as u32
        });
    } else if let Some(value) = setting.strip_prefix("transition-easing=") {
        replace(&mut slide.transition_easing, value);
    } else if let Some(value) = setting.strip_prefix("camera-framerate=") {
        slide.camera_framerate = value
            .parse::<i32>()
            .ok()
            .filter(|value| (0..=MAX_CAMERA_FRAMERATE).contains(value))
            .unwrap_or(0);
    } else if let Some(value) = setting.strip_prefix("camera-resolution=") {
        slide.camera_resolution = parse_resolution(value);
    } else if setting == "fill" {
        slide.background_scale = BackgroundScale::Fill;
    } else if setting == "fit" {
        slide.background_scale = BackgroundScale::Fit;
    } else if setting == "stretch" {
        slide.background_scale = BackgroundScale::Stretch;
    } else if setting == "unscaled" {
        slide.background_scale = BackgroundScale::Unscaled;
    } else if let Some(value) = setting.strip_prefix("bg-position=") {
        slide.background_position = match value {
            "top-left" => Gravity::TopLeft,
            "left" => Gravity::Left,
            "bottom-left" => Gravity::BottomLeft,
            "top-right" => Gravity::TopRight,
            "right" => Gravity::Right,
            "bottom-right" => Gravity::BottomRight,
            _ => Gravity::Center,
        };
    } else if setting == "no-markup" {
        slide.use_markup = false;
    } else if setting == "markup" {
        slide.use_markup = true;
    } else if let Some(gravity) = match setting {
        "center" => Some(Gravity::Center),
        "top" => Some(Gravity::Top),
        "bottom" => Some(Gravity::Bottom),
        "left" => Some(Gravity::Left),
        "right" => Some(Gravity::Right),
        "top-left" => Some(Gravity::TopLeft),
        "top-right" => Some(Gravity::TopRight),
        "bottom-left" => Some(Gravity::BottomLeft),
        "bottom-right" => Some(Gravity::BottomRight),
        _ => None,
    } {
        slide.text_position = gravity;
    } else {
        slide.background = Some(setting.into());
    }
}

fn parse_config(slide: &mut Slide, config: &str) {
    let mut rest = config;
    while let Some(open) = rest.find('[') {
        let candidate = &rest[open + 1..];
        let Some(close) = candidate.find([']', '\n']) else {
            break;
        };
        if candidate.as_bytes()[close] == b']' {
            parse_setting(slide, &candidate[..close]);
            rest = &candidate[close + 1..];
        } else {
            rest = &candidate[close..];
        }
    }
}

fn config_within_limits(config: &str) -> bool {
    let mut rest = config;
    while let Some(open) = rest.find('[') {
        let candidate = &rest[open + 1..];
        let Some(close) = candidate.find([']', '\n']) else {
            return candidate.len() <= MAX_SETTING_BYTES;
        };
        if close > MAX_SETTING_BYTES {
            return false;
        }
        rest = &candidate[close + 1..];
    }
    true
}

fn markup_depth_within_limit(text: &str) -> bool {
    let mut rest = text;
    let mut depth = 0_usize;
    while let Some(open) = rest.find('<') {
        let candidate = &rest[open + 1..];
        let Some(close) = candidate.find('>') else {
            break;
        };
        let tag = candidate[..close].trim();
        if tag.starts_with('/') {
            depth = depth.saturating_sub(1);
        } else if tag
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic())
            && !tag.ends_with('/')
        {
            depth += 1;
            if depth > MAX_MARKUP_DEPTH {
                return false;
            }
        }
        rest = &candidate[close + 1..];
    }
    true
}

fn is_color(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if lower.starts_with('#') && matches!(lower.len(), 4 | 5 | 7 | 9) {
        return lower[1..].bytes().all(|byte| byte.is_ascii_hexdigit());
    }
    lower.starts_with("rgb(")
        || lower.starts_with("rgba(")
        || [
            "transparent",
            "black",
            "white",
            "red",
            "green",
            "blue",
            "yellow",
            "gray",
            "grey",
            "orange",
            "purple",
            "pink",
            "brown",
            "cyan",
            "magenta",
        ]
        .contains(&lower.as_str())
}

fn sniff_video(base: Option<&Path>, background: &str) -> bool {
    let Some(path) = asset::resolve_local(base, background, asset::Access::Confined) else {
        return false;
    };
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut prefix = [0_u8; 12];
    let Ok(read) = file.read(&mut prefix) else {
        return false;
    };
    let prefix = &prefix[..read];
    prefix.starts_with(b"\x1aE\xdf\xa3")
        || prefix.get(4..8) == Some(b"ftyp")
        || prefix.starts_with(b"OggS")
        || prefix.starts_with(b"GIF8")
}

fn classify(slide: &mut Slide, base: Option<&Path>) {
    let Some(background) = slide.background.as_deref() else {
        slide.background_type = BackgroundType::None;
        return;
    };
    let lower = background.to_ascii_lowercase();
    let video_suffixes = [
        ".avi", ".ogg", ".ogv", ".mpg", ".flv", ".mpeg", ".mov", ".mp4", ".m4v", ".wmv", ".webm",
        ".mkv", ".3gp", ".gif", ".ts", ".mts", ".m2ts", ".m2v", ".mxf", ".vob", ".m3u8",
    ];
    slide.background_type = if lower == "camera" {
        BackgroundType::Camera
    } else if is_color(background) {
        BackgroundType::Color
    } else if lower.ends_with(".svg") {
        BackgroundType::Svg
    } else if video_suffixes.iter().any(|suffix| lower.ends_with(suffix))
        || sniff_video(base, background)
    {
        BackgroundType::Video
    } else {
        BackgroundType::Image
    };
}

fn finish(
    slides: &mut Vec<Slide>,
    mut slide: Slide,
    text: &mut String,
    notes: &mut String,
    alt: &mut String,
    base: Option<&Path>,
) -> Result<(), ParseError> {
    if text.len() > MAX_SLIDE_TEXT_BYTES {
        return Err(ParseError::SlideTextTooLong);
    }
    if notes.len() > MAX_NOTES_BYTES {
        return Err(ParseError::NotesTooLong);
    }
    if alt.len() > MAX_VISUAL_DESCRIPTION_BYTES {
        return Err(ParseError::VisualDescriptionTooLong);
    }
    if slide.use_markup && !markup_depth_within_limit(text) {
        return Err(ParseError::MarkupTooDeep);
    }
    slide.text = Some(text.trim_matches('\n').into());
    if !notes.is_empty() {
        slide.speaker_notes = Some(std::mem::take(notes));
    }
    if !alt.is_empty() {
        slide.visual_description = Some(std::mem::take(alt));
    }
    classify(&mut slide, base);
    slides.push(slide);
    text.clear();
    Ok(())
}

fn parse_with_stage_color(
    source: &str,
    path: Option<&Path>,
    ignore_comments: bool,
    stage_color: &str,
) -> Result<Presentation, ParseError> {
    if source.len() as u64 > MAX_PRESENTATION_BYTES {
        return Err(ParseError::TooLarge);
    }
    if source.contains('\0') {
        return Err(ParseError::NulByte);
    }
    let bytes = source.as_bytes();
    let mut defaults = Slide {
        stage_color: stage_color.into(),
        ..Slide::default()
    };
    let mut slide = defaults.clone();
    let mut slides = Vec::new();
    let mut text = String::new();
    let mut notes = String::new();
    let mut alt = String::new();
    let mut position = 0;
    let mut start_of_line = true;
    let mut got_config = false;

    while position <= bytes.len() {
        let current = bytes.get(position).copied().unwrap_or(0);
        if current == b'\\' && position + 1 < bytes.len() {
            position += 1;
            let character = source[position..]
                .chars()
                .next()
                .expect("an escaped character follows the backslash");
            text.push(character);
            start_of_line = false;
            position += character.len_utf8();
        } else if current == b'\n' {
            text.push('\n');
            start_of_line = true;
            position += 1;
        } else if (current == b'-' && start_of_line) || current == 0 {
            let line_end = if current == 0 {
                position
            } else {
                bytes[position..]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(bytes.len(), |offset| position + offset)
            };
            let settings = &source[position..line_end];
            if !config_within_limits(settings) {
                return Err(ParseError::SettingTooLong);
            }
            position = if line_end < bytes.len() {
                line_end + 1
            } else {
                line_end + usize::from(current == 0)
            };
            let mut next = defaults.clone();
            parse_config(&mut next, settings);
            if !got_config {
                if !config_within_limits(&text) {
                    return Err(ParseError::SettingTooLong);
                }
                parse_config(&mut defaults, &text);
                slide = defaults.clone();
                parse_config(&mut slide, settings);
                got_config = true;
            } else {
                if slides.len() >= MAX_SLIDES {
                    return Err(ParseError::TooManySlides);
                }
                finish(&mut slides, slide, &mut text, &mut notes, &mut alt, path)?;
                slide = next;
            }
            text.clear();
            notes.clear();
            alt.clear();
            start_of_line = true;
            if current == 0 {
                break;
            }
        } else if current == b'#' && start_of_line {
            position += 1;
            let is_alt = source[position..].starts_with("@alt:");
            if is_alt {
                position += "@alt:".len();
            }
            let line_end = bytes[position..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |offset| position + offset);
            let target = if is_alt { &mut alt } else { &mut notes };
            if is_alt || !ignore_comments {
                target.push_str(&source[position..line_end]);
                target.push('\n');
            }
            position = if line_end < bytes.len() {
                line_end + 1
            } else {
                line_end
            };
            start_of_line = true;
        } else {
            let character = source[position..]
                .chars()
                .next()
                .expect("valid UTF-8 position");
            text.push(character);
            position += character.len_utf8();
            start_of_line = false;
        }
    }

    if !got_config || slides.is_empty() {
        Err(ParseError::NoSlideSeparator)
    } else {
        Ok(Presentation {
            defaults,
            slides,
            source: source.into(),
            asset_access: asset::Access::Confined,
        })
    }
}

pub fn parse(
    source: &str,
    path: Option<&Path>,
    ignore_comments: bool,
) -> Result<Presentation, ParseError> {
    parse_with_stage_color(source, path, ignore_comments, "black")
}

pub fn first_changed_slide(old: &Presentation, new: &Presentation) -> usize {
    let mut separators = 0_usize;
    let mut start_of_line = true;
    for (old, new) in old.source.bytes().zip(new.source.bytes()) {
        if old != new {
            break;
        }
        if start_of_line && old == b'-' {
            separators = separators.saturating_add(1);
        }
        start_of_line = old == b'\n';
    }
    separators.saturating_sub(1)
}

pub fn load(path: &Path, ignore_comments: bool) -> Result<Presentation, ParseError> {
    let source = read_source(path)?;
    parse(&source, Some(path), ignore_comments)
}

pub fn load_for_pdf(path: &Path, ignore_comments: bool) -> Result<Presentation, ParseError> {
    let source = read_source(path)?;
    parse_with_stage_color(&source, Some(path), ignore_comments, "white")
}

pub fn read_source(path: &Path) -> Result<String, ParseError> {
    let bytes = read_bounded(path)?;
    let source = std::str::from_utf8(&bytes).map_err(ParseError::InvalidUtf8)?;
    if source.contains('\0') {
        return Err(ParseError::NulByte);
    }
    Ok(source.to_owned())
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, ParseError> {
    let file = std::fs::File::open(path).map_err(ParseError::Io)?;
    if file.metadata().map_err(ParseError::Io)?.len() > MAX_PRESENTATION_BYTES {
        return Err(ParseError::TooLarge);
    }
    let mut bytes = Vec::new();
    file.take(MAX_PRESENTATION_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(ParseError::Io)?;
    if bytes.len() as u64 > MAX_PRESENTATION_BYTES {
        Err(ParseError::TooLarge)
    } else {
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_fixture() {
        let source = include_str!("../../tests/fixtures/compatibility.pin");
        let presentation = parse(source, None, false).unwrap();
        assert_eq!(presentation.slides.len(), 3);
        assert_eq!(presentation.defaults.stage_color, "#112233");
        assert_eq!(presentation.defaults.text_position, Gravity::Bottom);
        assert_eq!(
            presentation.slides[0].speaker_notes.as_deref(),
            Some("First note\nSecond note\n")
        );
        assert_eq!(
            presentation.slides[1].background_type,
            BackgroundType::Image
        );
        assert!(!presentation.slides[1].use_markup);
        assert_eq!(
            presentation.slides[2].text.as_deref(),
            Some("-- this is text, not a separator\n# this is text, not a note")
        );
    }

    #[test]
    fn pdf_uses_white_default_stage_color() {
        let presentation = parse_with_stage_color("--\nPDF slide\n", None, false, "white").unwrap();
        assert_eq!(presentation.defaults.stage_color, "white");
        assert_eq!(presentation.slides[0].stage_color, "white");
    }

    #[test]
    fn per_slide_stage_color_is_not_treated_as_a_background_asset() {
        let presentation = parse("-- [stage-color=#3050d0]\nColour\n", None, false).unwrap();
        let slide = &presentation.slides[0];
        assert_eq!(slide.stage_color, "#3050d0");
        assert_eq!(slide.background, None);
        assert_eq!(slide.background_type, BackgroundType::None);
    }

    #[test]
    fn comments_and_visual_descriptions() {
        let source = "-- [photo.jpg]\nAudience text\n#@alt:One.\n#Speaker reminder\n";
        let normal = parse(source, None, false).unwrap();
        let ignored = parse(source, None, true).unwrap();
        assert_eq!(
            normal.slides[0].speaker_notes.as_deref(),
            Some("Speaker reminder\n")
        );
        assert_eq!(ignored.slides[0].speaker_notes, None);
        assert_eq!(
            ignored.slides[0].visual_description.as_deref(),
            Some("One.\n")
        );
    }

    #[test]
    fn escaped_non_ascii_characters_preserve_utf8_boundaries() {
        for character in ['é', '界', '🦀'] {
            let source = format!("--\n\\{character}\n");
            let presentation = parse(&source, None, false).expect("valid escaped Unicode");
            let expected = character.to_string();
            assert_eq!(
                presentation.slides[0].text.as_deref(),
                Some(expected.as_str())
            );
        }
    }

    #[test]
    fn embedded_nul_is_rejected_instead_of_truncating_the_deck() {
        assert!(matches!(
            parse("--\nBefore\0After\n", None, false),
            Err(ParseError::NulByte)
        ));
    }

    #[test]
    fn unsafe_numeric_settings_fall_back_or_are_bounded() {
        let presentation = parse(
            "-- [duration=NaN] [shading-opacity=2] [transition-duration=999999] [camera-framerate=-1] [camera-resolution=99999x1]\nSlide\n",
            None,
            false,
        )
        .unwrap();
        let slide = &presentation.slides[0];
        assert_eq!(slide.duration, 0.0);
        assert_eq!(slide.shading_opacity, 0.0);
        assert_eq!(slide.transition_duration_ms, MAX_TRANSITION_DURATION_MS);
        assert_eq!(slide.camera_framerate, 0);
        assert_eq!(slide.camera_resolution, Resolution::default());
    }

    #[test]
    fn source_size_and_slide_count_have_explicit_limits() {
        let oversized = "x".repeat(MAX_PRESENTATION_BYTES as usize + 1);
        assert!(matches!(
            parse(&oversized, None, false),
            Err(ParseError::TooLarge)
        ));
        let pathological = "--\n".repeat(MAX_SLIDES + 2);
        assert!(matches!(
            parse(&pathological, None, false),
            Err(ParseError::TooManySlides)
        ));
    }

    #[test]
    fn per_slide_content_and_markup_limits_are_enforced() {
        let setting = format!("-- [font={}]\nSlide\n", "x".repeat(MAX_SETTING_BYTES + 1));
        assert!(matches!(
            parse(&setting, None, false),
            Err(ParseError::SettingTooLong)
        ));

        let text = format!("--\n{}\n", "x".repeat(MAX_SLIDE_TEXT_BYTES + 1));
        assert!(matches!(
            parse(&text, None, false),
            Err(ParseError::SlideTextTooLong)
        ));

        let notes = format!("--\nSlide\n#{}\n", "x".repeat(MAX_NOTES_BYTES + 1));
        assert!(matches!(
            parse(&notes, None, false),
            Err(ParseError::NotesTooLong)
        ));

        let description = format!(
            "--\nSlide\n#@alt:{}\n",
            "x".repeat(MAX_VISUAL_DESCRIPTION_BYTES + 1)
        );
        assert!(matches!(
            parse(&description, None, false),
            Err(ParseError::VisualDescriptionTooLong)
        ));

        let markup = format!("--\n{}text\n", "<b>".repeat(MAX_MARKUP_DEPTH + 1));
        assert!(matches!(
            parse(&markup, None, false),
            Err(ParseError::MarkupTooDeep)
        ));
    }

    #[test]
    fn background_and_transition_settings() {
        let source = "[transition=slide][transition-duration=900]\n-- [movie.MP4]\nVideo\n-- [art.svg] [transition-layer=text]\nSVG\n";
        let presentation = parse(source, None, false).unwrap();
        assert_eq!(
            presentation.slides[0].background_type,
            BackgroundType::Video
        );
        assert_eq!(presentation.slides[1].background_type, BackgroundType::Svg);
        assert_eq!(presentation.slides[0].transition_duration_ms, 900);
        assert_eq!(
            presentation.slides[1].transition_layer,
            TransitionLayer::Text
        );
    }

    #[test]
    fn reload_selects_the_first_changed_slide() {
        let old = parse("--\nOne\n--\nTwo\n--\nThree\n", None, false).unwrap();
        let second = parse("--\nOne\n--\nChanged\n--\nThree\n", None, false).unwrap();
        let first = parse("--\nChanged\n--\nTwo\n--\nThree\n", None, false).unwrap();
        assert_eq!(first_changed_slide(&old, &second), 1);
        assert_eq!(first_changed_slide(&old, &first), 0);
    }

    #[test]
    fn source_is_retained_but_not_serialized() {
        let presentation = parse("--\nSlide\n", None, false).unwrap();
        assert_eq!(presentation.source, "--\nSlide\n");
        assert!(
            !serde_json::to_string(&presentation)
                .unwrap()
                .contains("source")
        );
    }
}
