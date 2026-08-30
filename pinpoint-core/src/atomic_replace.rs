use rustix::fs::{CWD, RenameFlags, renameat_with};
use std::fmt;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Identity {
    device: u64,
    inode: u64,
    length: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Snapshot {
    Missing,
    Existing(Identity),
}

#[derive(Debug)]
pub enum Error {
    Changed,
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Changed => formatter.write_str("destination changed before replacement"),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {}

fn identity(metadata: &fs::Metadata) -> Identity {
    Identity {
        device: metadata.dev(),
        inode: metadata.ino(),
        length: metadata.len(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
    }
}

pub fn snapshot(path: &Path) -> Result<Snapshot, std::io::Error> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Snapshot::Existing(identity(&metadata))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Snapshot::Missing),
        Err(error) => Err(error),
    }
}

/// Atomically install `temporary` only when `destination` is still the file
/// represented by `expected`. On Linux the exchange itself closes the final
/// check/rename gap: the displaced destination is verified after the swap and
/// restored if it differs.
pub fn commit_if_unchanged(
    temporary: &Path,
    destination: &Path,
    expected: Snapshot,
) -> Result<(), Error> {
    match expected {
        Snapshot::Missing => {
            renameat_with(CWD, temporary, CWD, destination, RenameFlags::NOREPLACE).map_err(
                |error| {
                    if error == rustix::io::Errno::EXIST {
                        Error::Changed
                    } else {
                        Error::Io(error.into())
                    }
                },
            )
        }
        Snapshot::Existing(expected_identity) => {
            renameat_with(CWD, temporary, CWD, destination, RenameFlags::EXCHANGE).map_err(
                |error| {
                    if error == rustix::io::Errno::NOENT {
                        Error::Changed
                    } else {
                        Error::Io(error.into())
                    }
                },
            )?;
            let displaced_matches = snapshot(temporary)
                .map_err(Error::Io)
                .is_ok_and(|snapshot| snapshot == Snapshot::Existing(expected_identity));
            if displaced_matches {
                fs::remove_file(temporary).map_err(Error::Io)?;
                return Ok(());
            }
            renameat_with(CWD, temporary, CWD, destination, RenameFlags::EXCHANGE)
                .map_err(|error| Error::Io(error.into()))?;
            Err(Error::Changed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn directory() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pinpoint-atomic-replace-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn a_changed_existing_destination_is_restored() {
        let directory = directory();
        let destination = directory.join("deck.pin");
        let temporary = directory.join("new.tmp");
        fs::write(&destination, "original").unwrap();
        let expected = snapshot(&destination).unwrap();
        fs::write(&temporary, "generated").unwrap();
        fs::write(&destination, "external edit").unwrap();
        assert!(matches!(
            commit_if_unchanged(&temporary, &destination, expected),
            Err(Error::Changed)
        ));
        assert_eq!(fs::read_to_string(&destination).unwrap(), "external edit");
        fs::remove_file(temporary).unwrap();
        fs::remove_file(destination).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn a_new_destination_is_never_overwritten() {
        let directory = directory();
        let destination = directory.join("slides.pdf");
        let temporary = directory.join("new.tmp");
        let expected = snapshot(&destination).unwrap();
        fs::write(&temporary, "generated").unwrap();
        fs::write(&destination, "external output").unwrap();
        assert!(matches!(
            commit_if_unchanged(&temporary, &destination, expected),
            Err(Error::Changed)
        ));
        assert_eq!(fs::read_to_string(&destination).unwrap(), "external output");
        fs::remove_file(temporary).unwrap();
        fs::remove_file(destination).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
