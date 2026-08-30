use crate::source::{DurationCountError, apply_durations};
use crate::{atomic_replace, atomic_replace::Snapshot};
use std::fmt;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Rehearsal {
    durations: Vec<f64>,
}

impl Rehearsal {
    pub fn new(slide_count: usize) -> Self {
        Self {
            durations: vec![0.0; slide_count],
        }
    }

    pub fn record(&mut self, slide: usize, seconds: f64) -> Result<(), RecordError> {
        if slide >= self.durations.len() {
            return Err(RecordError::UnknownSlide {
                slide,
                count: self.durations.len(),
            });
        }
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(RecordError::InvalidDuration(seconds));
        }
        self.durations[slide] += seconds;
        Ok(())
    }

    pub fn durations(&self) -> &[f64] {
        &self.durations
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RecordError {
    UnknownSlide { slide: usize, count: usize },
    InvalidDuration(f64),
}

impl fmt::Display for RecordError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSlide { slide, count } => {
                write!(
                    formatter,
                    "Slide {} is outside a {}-slide rehearsal",
                    slide + 1,
                    count
                )
            }
            Self::InvalidDuration(duration) => {
                write!(formatter, "Invalid rehearsal duration: {duration}")
            }
        }
    }
}

impl std::error::Error for RecordError {}

#[derive(Debug)]
pub enum FinishError {
    Read(std::io::Error),
    ChangedOnDisk,
    Durations(DurationCountError),
    Write(std::io::Error),
}

impl fmt::Display for FinishError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => write!(formatter, "Unable to read presentation: {error}"),
            Self::ChangedOnDisk => {
                formatter.write_str("The presentation changed on disk during rehearsal")
            }
            Self::Durations(error) => error.fmt(formatter),
            Self::Write(error) => write!(formatter, "Unable to save rehearsal timings: {error}"),
        }
    }
}

impl std::error::Error for FinishError {}

/// Apply a completed rehearsal only if the source is still the snapshot that
/// was presented. This mirrors Pinpoint's deliberately conservative writeback:
/// another editor wins rather than having its changes silently overwritten.
pub fn finish_to_path(
    path: &Path,
    source_snapshot: &str,
    rehearsal: &Rehearsal,
) -> Result<String, FinishError> {
    finish_to_path_before_commit(path, source_snapshot, rehearsal, || {})
}

fn finish_to_path_before_commit(
    path: &Path,
    source_snapshot: &str,
    rehearsal: &Rehearsal,
    before_commit: impl FnOnce(),
) -> Result<String, FinishError> {
    let disk_snapshot = atomic_replace::snapshot(path).map_err(FinishError::Read)?;
    let current_source = fs::read_to_string(path).map_err(FinishError::Read)?;
    if current_source != source_snapshot
        || atomic_replace::snapshot(path).map_err(FinishError::Read)? != disk_snapshot
        || matches!(disk_snapshot, Snapshot::Missing)
    {
        return Err(FinishError::ChangedOnDisk);
    }
    let updated_source =
        apply_durations(source_snapshot, rehearsal.durations()).map_err(FinishError::Durations)?;
    let temporary = path.with_extension(format!("rehearsal-{}.tmp", std::process::id()));
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(FinishError::Write)?;
    if let Err(error) = file
        .write_all(updated_source.as_bytes())
        .and_then(|()| file.sync_all())
    {
        let _ = fs::remove_file(&temporary);
        return Err(FinishError::Write(error));
    }
    before_commit();
    if let Err(error) = atomic_replace::commit_if_unchanged(&temporary, path, disk_snapshot) {
        let _ = fs::remove_file(&temporary);
        return match error {
            atomic_replace::Error::Changed => Err(FinishError::ChangedOnDisk),
            atomic_replace::Error::Io(error) => Err(FinishError::Write(error)),
        };
    }
    Ok(updated_source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_path(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "pinpoint-{name}-{}-{nonce}.pin",
            std::process::id()
        ))
    }

    #[test]
    fn records_revisits_and_rejects_invalid_samples() {
        let mut rehearsal = Rehearsal::new(2);
        rehearsal.record(0, 1.25).unwrap();
        rehearsal.record(0, 0.75).unwrap();
        rehearsal.record(1, 4.5).unwrap();
        assert_eq!(rehearsal.durations(), &[2.0, 4.5]);
        assert!(matches!(
            rehearsal.record(2, 1.0),
            Err(RecordError::UnknownSlide { .. })
        ));
        assert!(matches!(
            rehearsal.record(1, f64::NAN),
            Err(RecordError::InvalidDuration(_))
        ));
    }

    #[test]
    fn finish_preserves_source_and_detects_external_change() {
        let source = "[duration=30] # default\n-- [duration=1.25]\nFirst\n-- [top]\nSecond\n";
        let path = temporary_path("rehearsal");
        fs::write(&path, source).unwrap();
        let mut rehearsal = Rehearsal::new(2);
        rehearsal.record(0, 2.5).unwrap();
        rehearsal.record(1, 4.75).unwrap();
        let updated = finish_to_path(&path, source, &rehearsal).unwrap();
        assert_eq!(
            updated,
            "[duration=30] # default\n-- [duration=2.5]\nFirst\n-- [top] [duration=4.75]\nSecond\n"
        );

        fs::write(&path, "--\nChanged elsewhere\n").unwrap();
        assert!(matches!(
            finish_to_path(&path, &updated, &rehearsal),
            Err(FinishError::ChangedOnDisk)
        ));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "--\nChanged elsewhere\n"
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn external_change_in_the_final_commit_window_wins() {
        let source = "--\nOriginal\n";
        let path = temporary_path("rehearsal-race");
        fs::write(&path, source).unwrap();
        let rehearsal = Rehearsal::new(1);
        let result = finish_to_path_before_commit(&path, source, &rehearsal, || {
            fs::write(&path, "--\nExternal edit\n").unwrap();
        });
        assert!(matches!(result, Err(FinishError::ChangedOnDisk)));
        assert_eq!(fs::read_to_string(&path).unwrap(), "--\nExternal edit\n");
        fs::remove_file(path).unwrap();
    }
}
