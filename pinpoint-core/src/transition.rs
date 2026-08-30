use crate::presentation::{Slide, TransitionDirection, TransitionLayer, TransitionMode};
use serde::Serialize;
use serde_json::Value;
use std::fmt;
use std::io::Read;
use std::path::Path;

const MAX_LEGACY_TRANSITION_BYTES: u64 = 1024 * 1024;
const MAX_LEGACY_ENTRIES: usize = 10_000;
const MAX_LEGACY_DURATION_MS: u32 = 60_000;

const BUILTINS: &[&str] = &[
    "fade",
    "slide-left",
    "slide-up",
    "slide-in-left",
    "text-slide-left",
    "text-slide-up",
    "text-slide-down",
    "spin",
    "spin-bg",
    "spin-text",
    "sheet",
    "swing",
    "page-curl",
    "page-curl-both",
    "action",
    "slide",
    "zoom",
    "scale",
    "flip",
];

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct LayerState {
    pub x: f32,
    pub y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub angle: f32,
    pub angle_x: f32,
    pub angle_y: f32,
    pub opacity: f32,
}

impl Default for LayerState {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            angle: 0.0,
            angle_x: 0.0,
            angle_y: 0.0,
            opacity: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct TransitionState {
    pub actor: LayerState,
    pub background: LayerState,
    pub midground: LayerState,
    pub foreground: LayerState,
}

#[derive(Debug)]
pub enum LegacyError {
    Io(std::io::Error),
    Json(serde_json::Error),
    InvalidData(&'static str),
    NotSupported,
}

impl fmt::Display for LegacyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Json(error) => error.fmt(formatter),
            Self::InvalidData(message) => formatter.write_str(message),
            Self::NotSupported => formatter.write_str(
                "The transition has no supported actor, background, midground, or foreground properties",
            ),
        }
    }
}

impl std::error::Error for LegacyError {}

#[derive(Clone, Debug, PartialEq)]
pub struct LegacyTransition {
    states: [TransitionState; 3],
    durations: [u32; 3],
    easing: [Option<String>; 3],
    pub unsupported_entries: u32,
}

fn state_index(name: &str) -> Option<usize> {
    match name {
        "pre" => Some(0),
        "show" => Some(1),
        "post" => Some(2),
        _ => None,
    }
}

fn layer_named<'a>(state: &'a mut TransitionState, name: &str) -> Option<&'a mut LayerState> {
    match name {
        "actor" => Some(&mut state.actor),
        "background" => Some(&mut state.background),
        "midground" => Some(&mut state.midground),
        "foreground" => Some(&mut state.foreground),
        _ => None,
    }
}

fn set_property(layer: &mut LayerState, property: &str, value: f64) -> bool {
    if !value.is_finite() || value.abs() > 1_000_000.0 {
        return false;
    }
    let value = value as f32;
    match property {
        "x" => layer.x = value,
        "y" => layer.y = value,
        "scale-x" => layer.scale_x = value,
        "scale-y" => layer.scale_y = value,
        "rotation-angle-z" => layer.angle = value,
        "rotation-angle-x" => layer.angle_x = value,
        "rotation-angle-y" => layer.angle_y = value,
        "opacity" => layer.opacity = (value / 255.0).clamp(0.0, 1.0),
        _ => return false,
    }
    true
}

fn non_negative_u32(value: Option<i64>, default: u32) -> u32 {
    value.map_or(default, |value| {
        value.clamp(0, MAX_LEGACY_DURATION_MS.into()) as u32
    })
}

impl LegacyTransition {
    pub fn from_json(source: &str) -> Result<Self, LegacyError> {
        if source.len() as u64 > MAX_LEGACY_TRANSITION_BYTES {
            return Err(LegacyError::InvalidData(
                "Legacy transition JSON exceeds the 1 MiB safety limit",
            ));
        }
        let root: Value = serde_json::from_str(source).map_err(LegacyError::Json)?;
        let objects = root.as_array().ok_or(LegacyError::InvalidData(
            "Legacy transition JSON must contain a top-level array",
        ))?;
        if objects.len() > MAX_LEGACY_ENTRIES {
            return Err(LegacyError::InvalidData(
                "Legacy transition JSON contains too many entries",
            ));
        }
        let unsupported_entries = objects
            .iter()
            .filter(|object| object.get("effects").is_some())
            .count() as u32;
        let state_object = objects
            .iter()
            .find(|object| object.get("type").and_then(Value::as_str) == Some("ClutterState"))
            .ok_or(LegacyError::InvalidData(
                "Legacy transition JSON has no ClutterState transitions",
            ))?;
        let transitions = state_object
            .get("transitions")
            .and_then(Value::as_array)
            .ok_or(LegacyError::InvalidData(
                "Legacy transition JSON has no ClutterState transitions",
            ))?;
        if transitions.len() > MAX_LEGACY_ENTRIES {
            return Err(LegacyError::InvalidData(
                "Legacy transition JSON contains too many transitions",
            ));
        }
        let default_duration =
            non_negative_u32(state_object.get("duration").and_then(Value::as_i64), 1000);
        let mut result = Self {
            states: [TransitionState::default(); 3],
            durations: [default_duration; 3],
            easing: [None, None, None],
            unsupported_entries,
        };
        let mut recognized = 0;

        for transition in transitions {
            let Some(state) = transition
                .get("target")
                .and_then(Value::as_str)
                .and_then(state_index)
            else {
                continue;
            };
            result.durations[state] = non_negative_u32(
                transition.get("duration").and_then(Value::as_i64),
                default_duration,
            );
            let Some(keys) = transition.get("keys").and_then(Value::as_array) else {
                continue;
            };
            if keys.len() > MAX_LEGACY_ENTRIES {
                return Err(LegacyError::InvalidData(
                    "Legacy transition JSON contains too many keys",
                ));
            }
            for key in keys {
                let Some(key) = key.as_array() else {
                    result.unsupported_entries += 1;
                    continue;
                };
                if key.len() < 4 {
                    result.unsupported_entries += 1;
                    continue;
                }
                let Some(layer_name) = key[0].as_str() else {
                    result.unsupported_entries += 1;
                    continue;
                };
                let Some(property) = key[1].as_str() else {
                    result.unsupported_entries += 1;
                    continue;
                };
                let Some(value) = key[3].as_f64() else {
                    result.unsupported_entries += 1;
                    continue;
                };
                let Some(layer) = layer_named(&mut result.states[state], layer_name) else {
                    result.unsupported_entries += 1;
                    continue;
                };
                if set_property(layer, property, value) {
                    recognized += 1;
                    if result.easing[state].is_none() {
                        result.easing[state] = key[2].as_str().map(str::to_owned);
                    }
                } else {
                    result.unsupported_entries += 1;
                }
            }
        }
        if recognized == 0 {
            Err(LegacyError::NotSupported)
        } else {
            Ok(result)
        }
    }

    pub fn load(path: &Path) -> Result<Self, LegacyError> {
        let file = std::fs::File::open(path).map_err(LegacyError::Io)?;
        if file.metadata().map_err(LegacyError::Io)?.len() > MAX_LEGACY_TRANSITION_BYTES {
            return Err(LegacyError::InvalidData(
                "Legacy transition JSON exceeds the 1 MiB safety limit",
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_LEGACY_TRANSITION_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(LegacyError::Io)?;
        if bytes.len() as u64 > MAX_LEGACY_TRANSITION_BYTES {
            return Err(LegacyError::InvalidData(
                "Legacy transition JSON exceeds the 1 MiB safety limit",
            ));
        }
        let source = std::str::from_utf8(&bytes)
            .map_err(|_| LegacyError::InvalidData("Legacy transition JSON is not valid UTF-8"))?;
        Self::from_json(source)
    }

    pub fn duration(&self, incoming: bool, backwards: bool) -> u32 {
        if incoming {
            self.durations[1]
        } else if backwards {
            self.durations[0]
        } else {
            self.durations[2]
        }
    }

    pub fn calculate(&self, incoming: bool, backwards: bool, progress: f64) -> TransitionState {
        let (from, to) = if incoming {
            (if backwards { 2 } else { 0 }, 1)
        } else {
            (1, if backwards { 0 } else { 2 })
        };
        let progress = apply_easing(self.easing[to].as_deref(), progress.clamp(0.0, 1.0));
        interpolate_state(&self.states[from], &self.states[to], progress)
    }
}

fn interpolate_layer(from: LayerState, to: LayerState, progress: f64) -> LayerState {
    let interpolate = |from: f32, to: f32| from + (to - from) * progress as f32;
    LayerState {
        x: interpolate(from.x, to.x),
        y: interpolate(from.y, to.y),
        scale_x: interpolate(from.scale_x, to.scale_x),
        scale_y: interpolate(from.scale_y, to.scale_y),
        angle: interpolate(from.angle, to.angle),
        angle_x: interpolate(from.angle_x, to.angle_x),
        angle_y: interpolate(from.angle_y, to.angle_y),
        opacity: interpolate(from.opacity, to.opacity),
    }
}

fn interpolate_state(
    from: &TransitionState,
    to: &TransitionState,
    progress: f64,
) -> TransitionState {
    TransitionState {
        actor: interpolate_layer(from.actor, to.actor, progress),
        background: interpolate_layer(from.background, to.background, progress),
        midground: interpolate_layer(from.midground, to.midground, progress),
        foreground: interpolate_layer(from.foreground, to.foreground, progress),
    }
}

pub fn is_builtin(name: &str) -> bool {
    BUILTINS.contains(&name)
}

pub fn side_enabled(slide: &Slide, incoming: bool) -> bool {
    !matches!(
        (incoming, slide.transition_mode),
        (true, TransitionMode::Out) | (false, TransitionMode::In)
    )
}

fn apply_to_targets(
    state: &mut TransitionState,
    layer: TransitionLayer,
    mut apply: impl FnMut(&mut LayerState),
) {
    match layer {
        TransitionLayer::Background => apply(&mut state.background),
        TransitionLayer::Text => {
            apply(&mut state.midground);
            apply(&mut state.foreground);
        }
        TransitionLayer::Default | TransitionLayer::All => apply(&mut state.actor),
    }
}

fn set_spin_state(state: &mut LayerState, incoming: bool, progress: f64, direction: f32) {
    if incoming {
        state.angle = -360.0 * direction * (1.0 - progress) as f32;
        state.scale_x = 0.01 + 0.99 * progress as f32;
        state.scale_y = state.scale_x;
        state.opacity = progress as f32;
    } else {
        state.angle = 360.0 * direction * progress as f32;
        state.scale_x = 1.0 + 3.0 * progress as f32;
        state.scale_y = state.scale_x;
        state.opacity = 1.0 - progress as f32;
    }
}

pub fn builtin_state(
    slide: &Slide,
    incoming: bool,
    backwards: bool,
    progress: f64,
) -> TransitionState {
    let mut state = TransitionState::default();
    if !is_builtin(&slide.transition) || !side_enabled(slide, incoming) {
        return state;
    }
    let progress = apply_easing(Some(&slide.transition_easing), progress.clamp(0.0, 1.0));
    let mut direction = if backwards { -1.0 } else { 1.0 };
    if matches!(
        slide.transition_direction,
        TransitionDirection::Right | TransitionDirection::Down
    ) {
        direction *= -1.0;
    }
    let opacity = if incoming {
        progress as f32
    } else {
        1.0 - progress as f32
    };
    match slide.transition.as_str() {
        "fade" => apply_to_targets(&mut state, slide.transition_layer, |target| {
            target.opacity = opacity;
        }),
        "slide" => {
            let offset = if incoming {
                direction * (1.0 - progress) as f32 * 1024.0
            } else {
                -direction * progress as f32 * 1024.0
            };
            apply_to_targets(&mut state, slide.transition_layer, |target| {
                if matches!(
                    slide.transition_direction,
                    TransitionDirection::Left | TransitionDirection::Right
                ) {
                    target.x = offset;
                } else {
                    target.y = offset;
                }
            });
        }
        "zoom" | "scale" => {
            let scale = if incoming {
                0.8 + 0.2 * progress as f32
            } else {
                1.0 + 0.2 * progress as f32
            };
            apply_to_targets(&mut state, slide.transition_layer, |target| {
                target.scale_x = scale;
                target.scale_y = scale;
                target.opacity = opacity;
            });
        }
        "flip" => {
            let angle = if incoming {
                direction * 90.0 * (1.0 - progress) as f32
            } else {
                -direction * 90.0 * progress as f32
            };
            apply_to_targets(&mut state, slide.transition_layer, |target| {
                if matches!(
                    slide.transition_direction,
                    TransitionDirection::Left | TransitionDirection::Right
                ) {
                    target.angle_y = angle;
                } else {
                    target.angle_x = angle;
                }
                target.opacity = opacity;
            });
        }
        "spin" if slide.transition_layer != TransitionLayer::Default => {
            apply_to_targets(&mut state, slide.transition_layer, |target| {
                set_spin_state(target, incoming, progress, direction);
            });
        }
        "page-curl" | "page-curl-both" => state.actor.opacity = opacity,
        "slide-left" | "action" => {
            state.actor.x = if incoming {
                (if backwards { -1.0 } else { 1.0 }) * (1.0 - progress) as f32 * 1024.0
            } else {
                -(if backwards { -1.0 } else { 1.0 }) * progress as f32 * 1024.0
            };
        }
        "slide-up" => {
            state.actor.y = if incoming {
                (if backwards { -1.0 } else { 1.0 }) * (1.0 - progress) as f32 * 1024.0
            } else {
                -(if backwards { -1.0 } else { 1.0 }) * progress as f32 * 1024.0
            };
        }
        "slide-in-left" => {
            if incoming {
                state.actor.x =
                    (if backwards { -1.0 } else { 1.0 }) * (1.0 - progress) as f32 * 1024.0;
            }
            state.actor.opacity = opacity;
        }
        "text-slide-left" => {
            let offset = if incoming {
                (if backwards { -1.0 } else { 1.0 }) * (1.0 - progress) as f32 * 1024.0
            } else {
                -(if backwards { -1.0 } else { 1.0 }) * progress as f32 * 1024.0
            };
            state.foreground.x = offset;
            state.midground.x = offset;
            state.midground.opacity = opacity;
            state.background.opacity = opacity;
        }
        "text-slide-up" | "text-slide-down" => {
            let text_direction = if slide.transition == "text-slide-down" {
                -1.0
            } else {
                1.0
            };
            let navigation_direction = if backwards { -1.0 } else { 1.0 };
            let offset = if incoming {
                text_direction * navigation_direction * (1.0 - progress) as f32 * 1024.0
            } else {
                -text_direction * navigation_direction * progress as f32 * 1024.0
            };
            state.foreground.y = offset;
            state.midground.y = offset;
            state.background.opacity = opacity;
        }
        "spin-bg" => set_spin_state(&mut state.actor, incoming, progress, 1.0),
        "spin" => {
            set_spin_state(&mut state.background, incoming, progress, 1.0);
            state.actor.opacity = opacity;
        }
        "spin-text" => {
            set_spin_state(&mut state.foreground, incoming, progress, 1.0);
            state.midground = state.foreground;
            state.actor.opacity = opacity;
        }
        "sheet" | "swing" => {
            state.actor.opacity = opacity;
            if slide.transition == "sheet" {
                if !backwards {
                    state.actor.angle_x = if incoming {
                        90.0 * (1.0 - progress) as f32
                    } else {
                        0.0
                    };
                    state.actor.y = if incoming {
                        0.0
                    } else {
                        progress as f32 * 1024.0
                    };
                } else {
                    state.actor.angle_x = if incoming {
                        0.0
                    } else {
                        90.0 * progress as f32
                    };
                    state.actor.y = if incoming {
                        (1.0 - progress) as f32 * 1024.0
                    } else {
                        0.0
                    };
                }
            } else if !backwards {
                state.actor.angle_x = if incoming {
                    90.0 * (1.0 - progress) as f32
                } else {
                    -90.0 * progress as f32
                };
            } else {
                state.actor.angle_x = if incoming {
                    -90.0 * (1.0 - progress) as f32
                } else {
                    90.0 * progress as f32
                };
            }
        }
        _ => {}
    }
    state
}

pub fn apply_easing(name: Option<&str>, progress: f64) -> f64 {
    let Some(name) = name else { return progress };
    if name.eq_ignore_ascii_case("linear") {
        return progress;
    }
    let normalized: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    match normalized.as_str() {
        "easeinquad" => progress * progress,
        "easeoutquad" => 1.0 - (1.0 - progress).powi(2),
        "easeinoutquad" if progress < 0.5 => 2.0 * progress * progress,
        "easeinoutquad" => 1.0 - (-2.0 * progress + 2.0).powi(2) / 2.0,
        "easein" | "easeincubic" => progress.powi(3),
        "easeout" | "easeoutcubic" => 1.0 - (1.0 - progress).powi(3),
        "easeinout" | "easeinoutcubic" if progress < 0.5 => 4.0 * progress.powi(3),
        "easeinout" | "easeinoutcubic" => 1.0 - (-2.0 * progress + 2.0).powi(3) / 2.0,
        "easeoutquint" => 1.0 - (1.0 - progress).powi(5),
        _ => progress,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_match_current_contract() {
        assert!(is_builtin("page-curl"));
        assert!(is_builtin("flip"));
        assert!(!is_builtin("legacy-transition"));
    }

    #[test]
    fn easing_matches_current_contract() {
        assert!((apply_easing(Some("ease-in-quad"), 0.5) - 0.25).abs() < 0.0001);
        assert!((apply_easing(Some("ease-out-quad"), 0.5) - 0.75).abs() < 0.0001);
        assert!((apply_easing(Some("ease-in-out-cubic"), 0.75) - 0.9375).abs() < 0.0001);
        assert!((apply_easing(Some("ease-out-quint"), 0.5) - 0.96875).abs() < 0.0001);
        assert_eq!(apply_easing(Some("unknown"), 0.25), 0.25);
    }

    #[test]
    fn builtins_calculate_c_compatible_layer_states() {
        let slide = Slide {
            transition: "slide".into(),
            transition_direction: TransitionDirection::Down,
            transition_layer: TransitionLayer::Background,
            ..Slide::default()
        };
        let state = builtin_state(&slide, true, false, 0.25);
        assert_eq!(state.background.y, -768.0);
        assert_eq!(state.actor, LayerState::default());

        let text = Slide {
            transition: "text-slide-left".into(),
            ..Slide::default()
        };
        let state = builtin_state(&text, false, false, 0.25);
        assert_eq!(state.foreground.x, -256.0);
        assert_eq!(state.midground.opacity, 0.75);
        assert_eq!(state.background.opacity, 0.75);

        let flip = Slide {
            transition: "flip".into(),
            transition_direction: TransitionDirection::Right,
            ..Slide::default()
        };
        let state = builtin_state(&flip, true, false, 0.5);
        assert_eq!(state.actor.angle_y, -45.0);
        assert_eq!(state.actor.opacity, 0.5);
    }

    #[test]
    fn builtin_transition_modes_and_spin_match_the_contract() {
        let slide = Slide {
            transition: "spin".into(),
            transition_layer: TransitionLayer::Text,
            transition_mode: TransitionMode::In,
            ..Slide::default()
        };
        assert_eq!(
            builtin_state(&slide, false, false, 0.5),
            TransitionState::default()
        );
        let state = builtin_state(&slide, true, false, 0.5);
        assert_eq!(state.foreground.angle, -180.0);
        assert_eq!(state.midground.angle, -180.0);
        assert_eq!(state.foreground.opacity, 0.5);
    }

    #[test]
    fn legacy_fixture_matches_current_contract() {
        let transition = LegacyTransition::from_json(include_str!(
            "../../tests/fixtures/legacy-transition.json"
        ))
        .unwrap();
        assert_eq!(transition.duration(true, false), 750);
        assert_eq!(transition.duration(false, false), 1200);
        assert_eq!(transition.duration(false, true), 1000);
        let halfway = transition.calculate(true, false, 0.5);
        assert!((halfway.actor.x - 256.0).abs() < 0.001);
        assert!((halfway.actor.opacity - 0.75).abs() < 0.001);
        assert!((halfway.foreground.scale_x - 1.375).abs() < 0.001);
    }

    #[test]
    fn legacy_validation_matches_current_contract() {
        assert!(matches!(
            LegacyTransition::from_json("{}"),
            Err(LegacyError::InvalidData(_))
        ));
        assert!(matches!(
            LegacyTransition::from_json("[]"),
            Err(LegacyError::InvalidData(_))
        ));
        let unsupported = r#"[{"type":"ClutterState","transitions":[{"target":"pre","keys":[["actor","unsupported","linear",1]]}]}]"#;
        assert!(matches!(
            LegacyTransition::from_json(unsupported),
            Err(LegacyError::NotSupported)
        ));
    }
}
