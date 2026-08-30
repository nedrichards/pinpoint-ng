#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayCandidate {
    pub id: u32,
    pub builtin: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DisplaySelection {
    pub presenter: Option<u32>,
    pub audience: Option<u32>,
}

fn contains(candidates: &[DisplayCandidate], display: Option<u32>) -> bool {
    display.is_some_and(|display| candidates.iter().any(|candidate| candidate.id == display))
}

pub fn presenter(
    candidates: &[DisplayCandidate],
    audience: Option<u32>,
    preferred: Option<u32>,
    window_display: Option<u32>,
) -> Option<u32> {
    if preferred != audience && contains(candidates, preferred) {
        return preferred;
    }
    candidates
        .iter()
        .find(|candidate| Some(candidate.id) != audience && candidate.builtin)
        .or_else(|| {
            candidates.iter().find(|candidate| {
                Some(candidate.id) == window_display && Some(candidate.id) != audience
            })
        })
        .or_else(|| {
            candidates
                .iter()
                .find(|candidate| Some(candidate.id) != audience)
        })
        .map(|candidate| candidate.id)
}

pub fn audience(
    candidates: &[DisplayCandidate],
    presenter: Option<u32>,
    preferred: Option<u32>,
) -> Option<u32> {
    if preferred != presenter && contains(candidates, preferred) {
        return preferred;
    }
    candidates
        .iter()
        .find(|candidate| Some(candidate.id) != presenter && !candidate.builtin)
        .or_else(|| {
            candidates
                .iter()
                .find(|candidate| Some(candidate.id) != presenter)
        })
        .map(|candidate| candidate.id)
}

pub fn validate(candidates: &[DisplayCandidate], selection: &mut DisplaySelection) {
    if candidates.len() < 2 {
        *selection = DisplaySelection::default();
        return;
    }
    if !contains(candidates, selection.presenter) {
        selection.presenter = None;
    }
    if !contains(candidates, selection.audience) {
        selection.audience = None;
    }
    if selection.presenter == selection.audience {
        selection.audience = None;
    }
}

pub fn choose(
    candidates: &[DisplayCandidate],
    mut selection: DisplaySelection,
    window_display: Option<u32>,
) -> DisplaySelection {
    validate(candidates, &mut selection);
    if candidates.len() < 2 {
        return selection;
    }
    selection.presenter = presenter(
        candidates,
        selection.audience,
        selection.presenter,
        window_display,
    );
    selection.audience = audience(candidates, selection.presenter, selection.audience);
    selection
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTERNAL: DisplayCandidate = DisplayCandidate {
        id: 1,
        builtin: true,
    };
    const EXTERNAL: DisplayCandidate = DisplayCandidate {
        id: 2,
        builtin: false,
    };

    #[test]
    fn two_display_defaults_and_swap() {
        let candidates = [INTERNAL, EXTERNAL];
        let selection = choose(&candidates, DisplaySelection::default(), Some(EXTERNAL.id));
        assert_eq!(
            selection,
            DisplaySelection {
                presenter: Some(INTERNAL.id),
                audience: Some(EXTERNAL.id)
            }
        );
        assert_eq!(
            DisplaySelection {
                presenter: selection.audience,
                audience: selection.presenter
            },
            DisplaySelection {
                presenter: Some(EXTERNAL.id),
                audience: Some(INTERNAL.id)
            }
        );
    }

    #[test]
    fn three_displays_preserve_explicit_pair() {
        let candidates = [
            INTERNAL,
            EXTERNAL,
            DisplayCandidate {
                id: 3,
                builtin: false,
            },
        ];
        let selection = choose(
            &candidates,
            DisplaySelection {
                presenter: Some(3),
                audience: Some(2),
            },
            Some(1),
        );
        assert_eq!(selection.presenter, Some(3));
        assert_eq!(selection.audience, Some(2));
    }

    #[test]
    fn unplug_and_replug_recalculates_pair() {
        let mut selection = DisplaySelection {
            presenter: Some(1),
            audience: Some(2),
        };
        validate(&[INTERNAL], &mut selection);
        assert_eq!(selection, DisplaySelection::default());
        assert_eq!(
            choose(&[INTERNAL, EXTERNAL], selection, Some(1)),
            DisplaySelection {
                presenter: Some(1),
                audience: Some(2)
            }
        );
    }

    #[test]
    fn removed_member_does_not_poison_remaining_pair() {
        let candidates = [
            INTERNAL,
            DisplayCandidate {
                id: 3,
                builtin: false,
            },
        ];
        let selection = choose(
            &candidates,
            DisplaySelection {
                presenter: Some(1),
                audience: Some(2),
            },
            Some(1),
        );
        assert_eq!(selection.presenter, Some(1));
        assert_eq!(selection.audience, Some(3));
    }

    #[test]
    fn fallback_and_exhausted_paths() {
        let external = [
            DisplayCandidate {
                id: 1,
                builtin: false,
            },
            EXTERNAL,
        ];
        assert_eq!(presenter(&external, Some(1), None, Some(2)), Some(2));
        assert_eq!(presenter(&external[..1], Some(1), None, Some(1)), None);
        assert_eq!(audience(&[INTERNAL], Some(1), None), None);
        assert_eq!(
            choose(&[INTERNAL], DisplaySelection::default(), Some(1)),
            DisplaySelection::default()
        );
    }
}
