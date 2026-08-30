use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrustDecision {
    Allowed(String),
    ConfirmationRequired(String),
    ConfirmationPending,
}

#[derive(Debug, Default)]
pub struct Trust {
    trusted_revision: Option<u64>,
    pending: Option<(u64, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyError {
    NotPresenting,
    Empty,
    TooLong,
}

impl fmt::Display for PolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPresenting => {
                formatter.write_str("slide commands are available only while presenting")
            }
            Self::Empty => formatter.write_str("the current slide has no command"),
            Self::TooLong => formatter.write_str("the slide command exceeds 64 KiB"),
        }
    }
}

impl std::error::Error for PolicyError {}

/// Commands execute inside the Pinpoint Flatpak. The production and development
/// manifests do not expose the Flatpak service, so a deck cannot use
/// `flatpak-spawn --host`. Pinpoint never broadens filesystem or bus permissions
/// merely because a deck contains a command.
pub fn allow(presenting: bool, command: Option<&str>) -> Result<&str, PolicyError> {
    if !presenting {
        return Err(PolicyError::NotPresenting);
    }
    let command = command
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .ok_or(PolicyError::Empty)?;
    if command.len() > 64 * 1024 {
        Err(PolicyError::TooLong)
    } else {
        Ok(command)
    }
}

impl Trust {
    pub fn request(
        &mut self,
        presenting: bool,
        revision: u64,
        command: Option<&str>,
    ) -> Result<TrustDecision, PolicyError> {
        let command = allow(presenting, command)?.to_owned();
        if self.trusted_revision == Some(revision) {
            return Ok(TrustDecision::Allowed(command));
        }
        if self.pending.is_some() {
            return Ok(TrustDecision::ConfirmationPending);
        }
        self.pending = Some((revision, command.clone()));
        Ok(TrustDecision::ConfirmationRequired(command))
    }

    pub fn confirm(&mut self, revision: u64, command: &str) -> bool {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.0 == revision && pending.1 == command)
        {
            self.pending = None;
            self.trusted_revision = Some(revision);
            true
        } else {
            false
        }
    }

    pub fn dismiss(&mut self) {
        self.pending = None;
    }

    pub fn revoke(&mut self) -> bool {
        self.trusted_revision.take().is_some() || self.pending.take().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_need_an_active_presentation_and_content() {
        assert_eq!(allow(true, Some(" printf ready ")).unwrap(), "printf ready");
        assert_eq!(
            allow(false, Some("printf ready")),
            Err(PolicyError::NotPresenting)
        );
        assert_eq!(allow(true, Some("  ")), Err(PolicyError::Empty));
        assert_eq!(allow(true, None), Err(PolicyError::Empty));
        assert_eq!(
            allow(true, Some(&"x".repeat(64 * 1024 + 1))),
            Err(PolicyError::TooLong)
        );
    }

    #[test]
    fn commands_require_confirmation_for_each_content_revision() {
        let mut trust = Trust::default();
        assert_eq!(
            trust.request(true, 1, Some(" printf ready ")).unwrap(),
            TrustDecision::ConfirmationRequired("printf ready".into())
        );
        assert_eq!(
            trust.request(true, 1, Some("printf ready")).unwrap(),
            TrustDecision::ConfirmationPending
        );
        assert!(trust.confirm(1, "printf ready"));
        assert_eq!(
            trust.request(true, 1, Some("printf next")).unwrap(),
            TrustDecision::Allowed("printf next".into())
        );
        assert_eq!(
            trust.request(true, 2, Some("printf next")).unwrap(),
            TrustDecision::ConfirmationRequired("printf next".into())
        );
    }

    #[test]
    fn stale_confirmation_cannot_trust_changed_content() {
        let mut trust = Trust::default();
        trust.request(true, 4, Some("old command")).unwrap();
        assert!(!trust.confirm(5, "new command"));
        assert!(trust.revoke());
        assert_eq!(
            trust.request(true, 5, Some("new command")).unwrap(),
            TrustDecision::ConfirmationRequired("new command".into())
        );
    }
}
