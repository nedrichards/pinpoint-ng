use pinpoint_core::presentation::{Slide, TransitionMode};
use pinpoint_core::transition::{
    LegacyTransition, TransitionState, apply_easing, builtin_state, is_builtin,
};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurlSide {
    pub period: f64,
    pub angle: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageCurlPlan {
    pub previous: CurlSide,
    pub current: CurlSide,
    pub backwards: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerPlan {
    pub previous: TransitionState,
    pub current: TransitionState,
    pub previous_above: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TransitionPlan {
    Layers(Box<LayerPlan>),
    PageCurl(PageCurlPlan),
}

pub fn side_enabled(slide: &Slide, incoming: bool) -> bool {
    !matches!(
        (incoming, slide.transition_mode),
        (true, TransitionMode::Out) | (false, TransitionMode::In)
    )
}

pub fn duration_with_legacy(
    slide: &Slide,
    incoming: bool,
    backwards: bool,
    legacy: Option<&LegacyTransition>,
) -> Duration {
    if slide.transition.is_empty() || !side_enabled(slide, incoming) {
        return Duration::ZERO;
    }
    let milliseconds = if slide.transition_duration_ms > 0 {
        slide.transition_duration_ms
    } else {
        match slide.transition.as_str() {
            "fade" | "spin" | "spin-bg" | "spin-text" => 800,
            "page-curl" => 2000,
            _ => legacy.map_or(1000, |transition| transition.duration(incoming, backwards)),
        }
    };
    Duration::from_millis(milliseconds.into())
}

fn is_page_curl(name: &str) -> bool {
    matches!(name, "page-curl" | "page-curl-both")
}

fn uses_page_curl(slide: &Slide, incoming: bool) -> bool {
    side_enabled(slide, incoming) && is_page_curl(&slide.transition)
}

fn curl_period(name: &str, incoming: bool, backwards: bool, progress: f64) -> f64 {
    if !is_page_curl(name) {
        0.0
    } else if name == "page-curl-both" {
        if incoming { 1.0 - progress } else { progress }
    } else if backwards {
        if incoming { 1.0 - progress } else { 0.0 }
    } else if incoming {
        0.0
    } else {
        progress
    }
}

pub fn plan(previous: &Slide, current: &Slide, backwards: bool, progress: f64) -> TransitionPlan {
    plan_with_legacy(previous, current, backwards, progress, None, None)
}

pub fn plan_with_legacy(
    previous: &Slide,
    current: &Slide,
    backwards: bool,
    progress: f64,
    previous_legacy: Option<&LegacyTransition>,
    current_legacy: Option<&LegacyTransition>,
) -> TransitionPlan {
    let progress = progress.clamp(0.0, 1.0);
    if uses_page_curl(previous, false) || uses_page_curl(current, true) {
        let previous_progress = apply_easing(Some(&previous.transition_easing), progress);
        let current_progress = apply_easing(Some(&current.transition_easing), progress);
        return TransitionPlan::PageCurl(PageCurlPlan {
            previous: CurlSide {
                period: if uses_page_curl(previous, false) {
                    curl_period(&previous.transition, false, backwards, previous_progress)
                } else {
                    0.0
                },
                angle: if previous.transition == "page-curl-both" {
                    15.0
                } else {
                    0.0
                },
            },
            current: CurlSide {
                period: if uses_page_curl(current, true) {
                    curl_period(&current.transition, true, backwards, current_progress)
                } else {
                    0.0
                },
                angle: if current.transition == "page-curl-both" {
                    15.0
                } else {
                    0.0
                },
            },
            backwards,
        });
    }

    let previous_enabled = side_enabled(previous, false) && !previous.transition.is_empty();
    let current_enabled = side_enabled(current, true) && !current.transition.is_empty();
    let layer_state =
        |slide: &Slide, incoming: bool, enabled: bool, legacy: Option<&LegacyTransition>| {
            if !enabled {
                return TransitionState::default();
            }
            if is_builtin(&slide.transition) {
                builtin_state(slide, incoming, backwards, progress)
            } else if let Some(legacy) = legacy {
                legacy.calculate(incoming, backwards, progress)
            } else {
                let mut state = TransitionState::default();
                state.actor.opacity = if incoming {
                    apply_easing(Some(&slide.transition_easing), progress) as f32
                } else {
                    1.0 - apply_easing(Some(&slide.transition_easing), progress) as f32
                };
                state
            }
        };
    TransitionPlan::Layers(Box::new(LayerPlan {
        previous: layer_state(previous, false, previous_enabled, previous_legacy),
        current: layer_state(current, true, current_enabled, current_legacy),
        previous_above: !current_enabled && previous_enabled,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slide(name: &str) -> Slide {
        Slide {
            transition: name.into(),
            ..Slide::default()
        }
    }

    #[test]
    fn duration_matches_c_page_curl_defaults() {
        assert_eq!(
            duration_with_legacy(&slide("page-curl"), false, false, None),
            Duration::from_millis(2000)
        );
        assert_eq!(
            duration_with_legacy(&slide("page-curl-both"), true, false, None),
            Duration::from_millis(1000)
        );
    }

    #[test]
    fn transition_mode_disables_the_opposite_side() {
        let mut incoming = slide("page-curl");
        incoming.transition_mode = TransitionMode::In;
        assert!(!side_enabled(&incoming, false));
        assert!(side_enabled(&incoming, true));
        assert_eq!(
            duration_with_legacy(&incoming, false, false, None),
            Duration::ZERO
        );
    }

    #[test]
    fn builtins_use_layer_transforms_instead_of_fading_everything() {
        let mut incoming = slide("slide");
        incoming.transition_direction = pinpoint_core::presentation::TransitionDirection::Up;
        let TransitionPlan::Layers(plan) = plan(&slide("fade"), &incoming, false, 0.25) else {
            panic!("expected layered transition plan");
        };
        assert_eq!(plan.current.actor.y, 768.0);
        assert_eq!(plan.current.actor.opacity, 1.0);
    }

    #[test]
    fn legacy_transitions_supply_their_own_layer_states() {
        let legacy = LegacyTransition::from_json(include_str!(
            "../../tests/fixtures/legacy-transition.json"
        ))
        .unwrap();
        let TransitionPlan::Layers(plan) = plan_with_legacy(
            &slide("legacy-transition"),
            &slide("fade"),
            false,
            0.5,
            Some(&legacy),
            None,
        ) else {
            panic!("expected layered transition plan");
        };
        assert!((plan.previous.actor.x + 256.0).abs() < 0.001);
    }

    #[test]
    fn forward_page_curl_turns_the_outgoing_page() {
        let plan = plan(&slide("page-curl"), &slide("fade"), false, 0.25);
        let TransitionPlan::PageCurl(plan) = plan else {
            panic!("expected curl plan");
        };
        assert_eq!(plan.previous.period, 0.25);
        assert_eq!(plan.current.period, 0.0);
        assert!(!plan.backwards);
    }

    #[test]
    fn backwards_page_curl_turns_the_incoming_page() {
        let plan = plan(&slide("fade"), &slide("page-curl"), true, 0.25);
        let TransitionPlan::PageCurl(plan) = plan else {
            panic!("expected curl plan");
        };
        assert_eq!(plan.previous.period, 0.0);
        assert_eq!(plan.current.period, 0.75);
        assert!(plan.backwards);
    }

    #[test]
    fn page_curl_both_turns_both_pages_at_an_angle() {
        let plan = plan(
            &slide("page-curl-both"),
            &slide("page-curl-both"),
            false,
            0.4,
        );
        let TransitionPlan::PageCurl(plan) = plan else {
            panic!("expected curl plan");
        };
        assert_eq!(
            plan.previous,
            CurlSide {
                period: 0.4,
                angle: 15.0
            }
        );
        assert_eq!(
            plan.current,
            CurlSide {
                period: 0.6,
                angle: 15.0
            }
        );
    }
}
