use serde::Serialize;
use std::fmt;
use std::path::Path;

use crate::presentation::MAX_SLIDES;

const SETTING_NAMES: &[&str] = &[
    "stage-color=",
    "font=",
    "notes-font=",
    "notes-font-size=",
    "text-color=",
    "text-align=",
    "shading-color=",
    "shading-opacity=",
    "duration=",
    "command=",
    "transition=",
    "transition-direction=",
    "transition-layer=",
    "transition-mode=",
    "transition-duration=",
    "transition-easing=",
    "camera-framerate=",
    "camera-resolution=",
    "bg-position=",
    "fill",
    "fit",
    "stretch",
    "unscaled",
    "center",
    "top",
    "bottom",
    "left",
    "right",
    "top-left",
    "top-right",
    "bottom-left",
    "bottom-right",
    "no-markup",
    "markup",
];

const ALIGN_VALUES: &[&str] = &["left", "center", "right"];
const GRAVITY_VALUES: &[&str] = &[
    "center",
    "top-left",
    "left",
    "bottom-left",
    "top-right",
    "right",
    "bottom-right",
];
const DIRECTION_VALUES: &[&str] = &["left", "right", "up", "down"];
const LAYER_VALUES: &[&str] = &["default", "all", "background", "text"];
const MODE_VALUES: &[&str] = &["both", "in", "out"];
const EASING_VALUES: &[&str] = &["linear", "ease-in", "ease-out", "ease-in-out"];
const MAX_DURATION_SECONDS: f64 = 24.0 * 60.0 * 60.0;
const MAX_TRANSITION_DURATION_MS: i64 = 60_000;
const MAX_CAMERA_DIMENSION: i64 = 16_384;
const MAX_CAMERA_FRAMERATE: i64 = 240;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub start: usize,
    pub end: usize,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceSlide {
    pub start: usize,
    pub separator_end: usize,
    pub end: usize,
    pub title: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Analysis {
    pub slides: Vec<SourceSlide>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Completion {
    pub label: String,
    pub insert_text: String,
    pub detail: String,
    pub replace_start: usize,
    pub cursor_back: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceEdit {
    pub replace_start: usize,
    pub replace_end: usize,
    pub insert_text: String,
}

/// Describe the source edit needed to attach an imported asset to the slide at
/// `cursor`. An asset chosen while editing a bracketed separator setting
/// replaces that setting. From slide body text it is appended to the current
/// separator, where the parser will treat it as a background rather than as
/// visible audience text.
pub fn imported_asset_edit(source: &str, cursor: usize, asset: &str) -> Option<SourceEdit> {
    if asset.is_empty()
        || asset.contains(['[', ']', '\n', '\r', '\0'])
        || cursor > source.len()
        || !source.is_char_boundary(cursor)
    {
        return None;
    }

    let analysis = analyze(source);
    let slide = analysis.slides.get(find_slide(&analysis, cursor))?;
    if (slide.start..=slide.separator_end).contains(&cursor) {
        let before_cursor = &source[slide.start..cursor];
        if let Some(relative_open) = before_cursor.rfind('[') {
            let open = slide.start + relative_open;
            if !source[open + 1..cursor].contains(']') {
                let close = source[cursor..slide.separator_end]
                    .find(']')
                    .map_or(cursor, |relative| cursor + relative + 1);
                return Some(SourceEdit {
                    replace_start: open,
                    replace_end: close,
                    insert_text: format!("[{asset}]"),
                });
            }
        }
    }

    Some(SourceEdit {
        replace_start: slide.separator_end,
        replace_end: slide.separator_end,
        insert_text: format!(" [{asset}]"),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurationCountError {
    expected: usize,
    received: usize,
}

impl fmt::Display for DurationCountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Expected {} rehearsal timings, received {}",
            self.expected, self.received
        )
    }
}

impl std::error::Error for DurationCountError {}

fn add_diagnostic(
    analysis: &mut Analysis,
    start: usize,
    end: usize,
    severity: DiagnosticSeverity,
    message: &str,
) {
    analysis.diagnostics.push(Diagnostic {
        start,
        end: end.max(start + 1),
        severity,
        message: message.to_owned(),
    });
}

fn setting_is_known(setting: &str) -> bool {
    SETTING_NAMES.iter().any(|name| {
        if name.ends_with('=') {
            setting.starts_with(name)
        } else {
            setting == *name
        }
    })
}

fn integer_in_range(value: &str, minimum: i64, maximum: i64) -> bool {
    value
        .parse::<i64>()
        .is_ok_and(|parsed| (minimum..=maximum).contains(&parsed))
}

fn analyze_setting(analysis: &mut Analysis, setting: &str, start: usize, end: usize) {
    if setting.contains('=') && !setting_is_known(setting) {
        add_diagnostic(
            analysis,
            start,
            end,
            DiagnosticSeverity::Warning,
            "Unknown setting; Pinpoint will treat it as a background",
        );
        return;
    }

    let warning = if let Some(value) = setting.strip_prefix("text-align=") {
        (!ALIGN_VALUES.contains(&value)).then_some("Text alignment must be left, center, or right")
    } else if let Some(value) = setting.strip_prefix("transition-direction=") {
        (!DIRECTION_VALUES.contains(&value)).then_some("Unknown transition direction")
    } else if let Some(value) = setting.strip_prefix("transition-layer=") {
        (!LAYER_VALUES.contains(&value)).then_some("Unknown transition layer")
    } else if let Some(value) = setting.strip_prefix("transition-mode=") {
        (!MODE_VALUES.contains(&value)).then_some("Unknown transition mode")
    } else if let Some(value) = setting.strip_prefix("duration=") {
        (!value.parse::<f64>().is_ok_and(|parsed| {
            parsed.is_finite() && (0.0..=MAX_DURATION_SECONDS).contains(&parsed)
        }))
        .then_some("Duration must be a non-negative number of seconds")
    } else if let Some(value) = setting.strip_prefix("shading-opacity=") {
        (!value
            .parse::<f64>()
            .is_ok_and(|parsed| parsed.is_finite() && (0.0..=1.0).contains(&parsed)))
        .then_some("Shading opacity must be between 0 and 1")
    } else if let Some(value) = setting.strip_prefix("transition-duration=") {
        (!integer_in_range(value, 0, MAX_TRANSITION_DURATION_MS))
            .then_some("Transition duration must be between 0 and 60000 milliseconds")
    } else if let Some(value) = setting.strip_prefix("camera-framerate=") {
        (!integer_in_range(value, 0, MAX_CAMERA_FRAMERATE))
            .then_some("Camera frame rate must be between 0 and 240")
    } else if let Some(value) = setting.strip_prefix("camera-resolution=") {
        let valid = value.split_once('x').is_some_and(|(width, height)| {
            integer_in_range(width, 1, MAX_CAMERA_DIMENSION)
                && integer_in_range(height, 1, MAX_CAMERA_DIMENSION)
        });
        (!valid).then_some("Camera resolution must be WIDTHxHEIGHT with dimensions up to 16384")
    } else {
        None
    };

    if let Some(message) = warning {
        add_diagnostic(analysis, start, end, DiagnosticSeverity::Warning, message);
    }
}

fn decode_markup(text: &str) -> Option<String> {
    let mut result = String::with_capacity(text.len());
    let mut in_tag = false;
    let mut entity = String::new();
    let mut in_entity = false;

    for character in text.chars() {
        if in_tag {
            if character == '>' {
                in_tag = false;
            }
            continue;
        }
        if in_entity {
            if character == ';' {
                result.push_str(match entity.as_str() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                });
                entity.clear();
                in_entity = false;
            } else {
                entity.push(character);
            }
            continue;
        }
        match character {
            '<' => in_tag = true,
            '&' => in_entity = true,
            _ => result.push(character),
        }
    }
    (!in_tag && !in_entity).then_some(result)
}

fn slide_title(source: &str, start: usize, end: usize, index: usize) -> String {
    for raw_line in source[start..end].lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let visible = line.strip_prefix('\\').unwrap_or(line);
        let plain = decode_markup(visible).unwrap_or_else(|| visible.to_owned());
        let plain = plain.trim();
        if !plain.is_empty() {
            return plain.chars().take(60).collect();
        }
    }
    format!("Slide {}", index + 1)
}

pub fn analyze(source: &str) -> Analysis {
    let mut analysis = Analysis::default();
    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut current: Option<usize> = None;

    while cursor < bytes.len() {
        let line_end = bytes[cursor..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |offset| cursor + offset);

        if bytes[cursor] == b'-' {
            if analysis.slides.len() >= MAX_SLIDES {
                add_diagnostic(
                    &mut analysis,
                    cursor,
                    line_end,
                    DiagnosticSeverity::Error,
                    "The presentation exceeds the slide safety limit",
                );
                break;
            }
            if let Some(current_index) = current {
                analysis.slides[current_index].end = cursor;
            }
            analysis.slides.push(SourceSlide {
                start: cursor,
                separator_end: line_end,
                end: bytes.len(),
                title: String::new(),
            });
            current = Some(analysis.slides.len() - 1);
        }

        let mut scan = cursor;
        while scan < line_end {
            let Some(open_offset) = bytes[scan..line_end].iter().position(|byte| *byte == b'[')
            else {
                break;
            };
            let open = scan + open_offset;
            let Some(close_offset) = bytes[open + 1..line_end]
                .iter()
                .position(|byte| *byte == b']')
            else {
                add_diagnostic(
                    &mut analysis,
                    open,
                    line_end,
                    DiagnosticSeverity::Warning,
                    "Setting is missing a closing bracket",
                );
                break;
            };
            let close = open + 1 + close_offset;
            analyze_setting(&mut analysis, &source[open + 1..close], open, close + 1);
            scan = close + 1;
        }
        cursor = if line_end < bytes.len() {
            line_end + 1
        } else {
            bytes.len()
        };
    }

    for (index, slide) in analysis.slides.iter_mut().enumerate() {
        let body = if slide.separator_end < bytes.len() {
            slide.separator_end + 1
        } else {
            bytes.len()
        };
        slide.title = slide_title(source, body, slide.end, index);
    }
    if analysis.slides.is_empty() {
        add_diagnostic(
            &mut analysis,
            0,
            bytes.len().max(1),
            DiagnosticSeverity::Error,
            "The presentation does not contain a slide separator",
        );
    }
    analysis
}

pub fn find_slide(analysis: &Analysis, offset: usize) -> usize {
    analysis
        .slides
        .iter()
        .rposition(|slide| offset >= slide.start)
        .unwrap_or(0)
}

pub fn setting_names() -> &'static [&'static str] {
    SETTING_NAMES
}

pub fn setting_values(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "text-align=" => Some(ALIGN_VALUES),
        "bg-position=" => Some(GRAVITY_VALUES),
        "transition-direction=" => Some(DIRECTION_VALUES),
        "transition-layer=" => Some(LAYER_VALUES),
        "transition-mode=" => Some(MODE_VALUES),
        "transition-easing=" => Some(EASING_VALUES),
        _ => None,
    }
}

pub fn list_assets(path: Option<&Path>) -> Vec<String> {
    let Some(directory) = path.and_then(Path::parent) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut assets = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_file())
                .and_then(|_| entry.file_name().into_string().ok())
        })
        .filter(|name| !name.ends_with(".pin"))
        .collect::<Vec<_>>();
    assets.sort();
    assets
}

fn add_completion(
    completions: &mut Vec<Completion>,
    label: impl Into<String>,
    insert_text: impl Into<String>,
    detail: impl Into<String>,
    replace_start: usize,
    cursor_back: usize,
    prefix: &str,
) {
    let label = label.into();
    let insert_text = insert_text.into();
    if !prefix.is_empty() && !label.starts_with(prefix) && !insert_text.starts_with(prefix) {
        return;
    }
    completions.push(Completion {
        label,
        insert_text,
        detail: detail.into(),
        replace_start,
        cursor_back,
    });
}

/// Return manual, context-aware completion candidates at a byte offset.  The
/// result is intentionally presentation-format knowledge only: no text is
/// generated and no filesystem mutation is attempted.
pub fn complete(source: &str, path: Option<&Path>, offset: usize) -> Vec<Completion> {
    const PANGO_TAGS: &[&str] = &[
        "markup", "span", "b", "big", "i", "s", "sub", "sup", "small", "tt", "u",
    ];
    let offset = offset.min(source.len());
    if !source.is_char_boundary(offset) {
        return Vec::new();
    }
    let line_start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
    let line_end = source[offset..]
        .find('\n')
        .map_or(source.len(), |index| offset + index);
    let line_before = &source[line_start..offset];
    let mut unmatched_bracket = None;
    for (index, character) in line_before.char_indices() {
        match character {
            '[' => unmatched_bracket = Some(line_start + index),
            ']' => unmatched_bracket = None,
            _ => {}
        }
    }
    let mut completions = Vec::new();
    if let Some(open) = unmatched_bracket {
        let token = &source[open + 1..offset];
        let has_closing = source[offset..line_end].starts_with(']');
        if let Some((name, prefix)) = token.split_once('=') {
            let name = format!("{name}=");
            if let Some(values) = setting_values(&name) {
                let replace_start = open + 1 + name.len();
                for value in values {
                    add_completion(
                        &mut completions,
                        *value,
                        format!("{value}{}", if has_closing { "" } else { "]" }),
                        "valid value",
                        replace_start,
                        0,
                        prefix,
                    );
                }
            }
        } else {
            for setting in setting_names() {
                add_completion(
                    &mut completions,
                    *setting,
                    *setting,
                    "slide setting",
                    open + 1,
                    0,
                    token,
                );
            }
            for asset in list_assets(path) {
                add_completion(
                    &mut completions,
                    format!("Asset: {asset}"),
                    format!("{asset}{}", if has_closing { "" } else { "]" }),
                    "relative asset",
                    open + 1,
                    0,
                    token,
                );
            }
        }
        return completions;
    }
    let mut unmatched_tag = None;
    for (index, character) in line_before.char_indices() {
        match character {
            '<' => unmatched_tag = Some(line_start + index),
            '>' => unmatched_tag = None,
            _ => {}
        }
    }
    if let Some(open) = unmatched_tag {
        let prefix = &source[open + 1..offset];
        for tag in PANGO_TAGS {
            let closing = format!("</{tag}>");
            add_completion(
                &mut completions,
                *tag,
                format!("{tag}>{closing}"),
                "Pango markup",
                open + 1,
                closing.chars().count(),
                prefix,
            );
        }
        return completions;
    }
    if source[line_start..].starts_with('#') {
        add_completion(
            &mut completions,
            "Visual description",
            "#@alt:",
            "speaker-view visual description",
            line_start,
            0,
            &source[line_start..offset],
        );
        return completions;
    }
    add_completion(
        &mut completions,
        "New slide",
        "\n--\n",
        "slide separator",
        offset,
        0,
        "",
    );
    add_completion(
        &mut completions,
        "Speaker note",
        "\n#",
        "speaker-view note",
        offset,
        0,
        "",
    );
    add_completion(
        &mut completions,
        "Visual description",
        "\n#@alt:",
        "speaker-view visual description",
        offset,
        0,
        "",
    );
    completions
}

pub fn completion_position_to_offset(source: &str, position: &str) -> Result<usize, String> {
    let Some((line, column)) = position.split_once(':') else {
        return Err("--complete-position must be a one-based LINE:COLUMN".into());
    };
    let line = line
        .parse::<usize>()
        .ok()
        .filter(|line| *line > 0)
        .ok_or_else(|| "--complete-position must be a one-based LINE:COLUMN".to_owned())?;
    let column = column
        .parse::<usize>()
        .ok()
        .filter(|column| *column > 0)
        .ok_or_else(|| "--complete-position must be a one-based LINE:COLUMN".to_owned())?;
    let mut line_start = 0;
    for _ in 1..line {
        let Some(newline) = source[line_start..].find('\n') else {
            return Err("completion line is outside the presentation".into());
        };
        line_start += newline + 1;
    }
    let line_end = source[line_start..]
        .find('\n')
        .map_or(source.len(), |index| line_start + index);
    let offset = line_start + column - 1;
    if offset > line_end || !source.is_char_boundary(offset) {
        return Err("completion column is outside the line".into());
    }
    Ok(offset)
}

fn format_duration(duration: f64) -> String {
    let mut formatted = format!("{duration:.3}");
    while formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    formatted
}

pub fn apply_durations(source: &str, durations: &[f64]) -> Result<String, DurationCountError> {
    let analysis = analyze(source);
    if analysis.slides.len() != durations.len() {
        return Err(DurationCountError {
            expected: analysis.slides.len(),
            received: durations.len(),
        });
    }

    let mut output = source.to_owned();
    for (slide, duration) in analysis.slides.iter().zip(durations).rev() {
        let formatted = format_duration(*duration);
        let line = &source[slide.start..slide.separator_end];
        if let Some(token_offset) = line.find("[duration=") {
            let value_offset = slide.start + token_offset + "[duration=".len();
            if let Some(close_offset) = source[value_offset..slide.separator_end].find(']') {
                output.replace_range(value_offset..value_offset + close_offset, &formatted);
                continue;
            }
        }
        output.insert_str(slide.separator_end, &format!(" [duration={formatted}]"));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_matches_current_contract() {
        let source = "[stage-color=black]\n-- [transition-mode=sideways] [duration=nope]\n<b>Opening</b>\n#notes\n-- [mystery=value]\n\\#Visible text\n";
        let analysis = analyze(source);
        assert_eq!(analysis.slides.len(), 2);
        assert_eq!(analysis.slides[0].title, "Opening");
        assert_eq!(analysis.slides[1].title, "#Visible text");
        assert_eq!(find_slide(&analysis, 0), 0);
        assert_eq!(find_slide(&analysis, analysis.slides[1].start + 2), 1);
        assert_eq!(analysis.diagnostics.len(), 3);
    }

    #[test]
    fn analysis_stops_before_pathological_outline_construction() {
        let source = "--\nSlide\n".repeat(MAX_SLIDES + 10);
        let analysis = analyze(&source);
        assert_eq!(analysis.slides.len(), MAX_SLIDES);
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Error
                && diagnostic.message.contains("slide safety limit")
        }));
    }

    #[test]
    fn incomplete_setting_is_reported() {
        let analysis = analyze("-- [duration=4\nSlide\n");
        assert_eq!(analysis.slides.len(), 1);
        assert_eq!(analysis.diagnostics.len(), 1);
        assert!(analysis.diagnostics[0].message.contains("closing bracket"));
    }

    #[test]
    fn unsafe_numeric_settings_are_diagnosed() {
        let analysis = analyze(
            "-- [duration=NaN] [shading-opacity=2] [transition-duration=999999] [camera-framerate=999] [camera-resolution=-1x0]\nSlide\n",
        );
        assert_eq!(analysis.diagnostics.len(), 5);
        assert!(
            analysis
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
        );
    }

    #[test]
    fn duration_writeback_preserves_source() {
        let source = "[duration=30]  # deliberately spaced defaults\n--   [top] [duration=1.25]   \nFirst\n# note\n-- [no-markup]\nSecond\n";
        let expected = "[duration=30]  # deliberately spaced defaults\n--   [top] [duration=5]   \nFirst\n# note\n-- [no-markup] [duration=8.375]\nSecond\n";
        assert_eq!(apply_durations(source, &[5.0, 8.375]).unwrap(), expected);
        assert!(apply_durations("--\nOne\n--\nTwo\n", &[1.0]).is_err());
    }

    #[test]
    fn completion_catalogue_matches_current_contract() {
        assert!(setting_names().contains(&"duration="));
        assert!(setting_names().contains(&"markup"));
        assert_eq!(setting_values("text-align="), Some(ALIGN_VALUES));
        assert_eq!(setting_values("bg-position="), Some(GRAVITY_VALUES));
        assert_eq!(
            setting_values("transition-direction="),
            Some(DIRECTION_VALUES)
        );
        assert_eq!(setting_values("transition-layer="), Some(LAYER_VALUES));
        assert_eq!(setting_values("transition-mode="), Some(MODE_VALUES));
        assert_eq!(setting_values("transition-easing="), Some(EASING_VALUES));
        assert_eq!(setting_values("duration="), None);
    }

    #[test]
    fn contextual_completion_replaces_only_the_active_token() {
        let source = "-- [text-align=c";
        let completions = complete(source, None, source.len());
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].label, "center");
        assert_eq!(completions[0].insert_text, "center]");
        assert_eq!(completions[0].replace_start, "-- [text-align=".len());

        let markup = complete("--\n<b", None, "--\n<b".len());
        assert_eq!(markup[0].label, "b");
        assert_eq!(markup[0].insert_text, "b></b>");
        assert_eq!(markup[0].cursor_back, 4);
    }

    #[test]
    fn imported_assets_are_formatted_on_the_current_slide_separator() {
        let source = "-- [fit]\nFirst slide\n--\nSecond slide\n";
        let cursor = source.find("Second slide").unwrap() + 3;
        let edit = imported_asset_edit(source, cursor, "photo.png").unwrap();
        assert_eq!(edit.replace_start, source.find("--\nSecond").unwrap() + 2);
        assert_eq!(edit.replace_start, edit.replace_end);
        assert_eq!(edit.insert_text, " [photo.png]");
    }

    #[test]
    fn imported_assets_replace_a_separator_setting_at_the_cursor() {
        let source = "-- [old.png] [fit]\nSlide\n";
        let cursor = source.find("old.png").unwrap() + 3;
        let edit = imported_asset_edit(source, cursor, "new image.png").unwrap();
        assert_eq!(&source[edit.replace_start..edit.replace_end], "[old.png]");
        assert_eq!(edit.insert_text, "[new image.png]");

        let source = "-- [par\nSlide\n";
        let edit = imported_asset_edit(source, source.find("par").unwrap() + 3, "new.png").unwrap();
        assert_eq!(&source[edit.replace_start..edit.replace_end], "[par");
        assert_eq!(edit.insert_text, "[new.png]");
    }

    #[test]
    fn imported_asset_names_must_be_representable_in_settings() {
        assert!(imported_asset_edit("--\nSlide\n", 0, "bad]name.png").is_none());
        assert!(imported_asset_edit("--\nSlide\n", 0, "bad\nname.png").is_none());
    }

    #[test]
    fn completion_position_is_one_based_and_character_safe() {
        assert_eq!(completion_position_to_offset("--\nOne\n", "2:2"), Ok(4));
        assert!(completion_position_to_offset("--\nOne\n", "3:9").is_err());
        assert!(completion_position_to_offset("--\nOne\n", "4:1").is_err());
    }
}
