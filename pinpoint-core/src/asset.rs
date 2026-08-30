use std::path::{Component, Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Access {
    #[default]
    Confined,
    Compatible,
}

fn normalize(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) => return None,
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Normal(component) => normalized.push(component),
        }
    }
    Some(normalized)
}

/// Resolve a deck asset without converting display-only host paths or allowing
/// non-local URI schemes into file APIs. Relative paths remain relative to the
/// presentation directory; the Flatpak document grant remains the filesystem
/// security boundary.
pub fn resolve_local(
    presentation_path: Option<&Path>,
    asset: &str,
    access: Access,
) -> Option<PathBuf> {
    if asset.is_empty() || asset.contains('\0') {
        return None;
    }
    if asset.contains("://") {
        if access == Access::Confined {
            return None;
        }
        let path = asset.strip_prefix("file://")?;
        if !path.starts_with('/') {
            return None;
        }
        return Some(PathBuf::from(path));
    }
    let path = Path::new(asset);
    if path.is_absolute() {
        return (access == Access::Compatible).then(|| path.to_path_buf());
    }
    let base = presentation_path
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."));
    if access == Access::Compatible {
        return Some(base.join(path));
    }
    let base = if base.is_absolute() {
        base.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(base)
    };
    let base = base.canonicalize().ok().or_else(|| normalize(&base))?;
    let candidate = normalize(&base.join(path))?;
    if !candidate.starts_with(&base) {
        return None;
    }
    if candidate.exists() {
        let canonical = candidate.canonicalize().ok()?;
        canonical.starts_with(&base).then_some(canonical)
    } else {
        Some(candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_only_local_assets() {
        let deck = Path::new("/documents/deck/talk.pin");
        assert_eq!(
            resolve_local(Some(deck), "images/title.png", Access::Confined),
            Some(PathBuf::from("/documents/deck/images/title.png"))
        );
        assert_eq!(
            resolve_local(
                Some(deck),
                "file:///documents/deck/title.png",
                Access::Compatible
            ),
            Some(PathBuf::from("/documents/deck/title.png"))
        );
        assert!(
            resolve_local(
                Some(deck),
                "https://example.invalid/a.png",
                Access::Compatible
            )
            .is_none()
        );
        assert!(resolve_local(Some(deck), "file://remote/a.png", Access::Compatible).is_none());
    }

    #[test]
    fn confined_assets_cannot_escape_the_deck_folder() {
        let deck = Path::new("/documents/deck/talk.pin");
        for asset in ["../secret.png", "/tmp/secret.png", "file:///tmp/secret.png"] {
            assert_eq!(resolve_local(Some(deck), asset, Access::Confined), None);
        }
        assert_eq!(
            resolve_local(Some(deck), "../shared.png", Access::Compatible),
            Some(PathBuf::from("/documents/deck/../shared.png"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn confined_assets_reject_symlinks_to_outside_files() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!("pinpoint-assets-{}", std::process::id()));
        let deck_dir = root.join("deck");
        std::fs::create_dir_all(&deck_dir).unwrap();
        let outside = root.join("outside.png");
        std::fs::write(&outside, b"outside").unwrap();
        symlink(&outside, deck_dir.join("linked.png")).unwrap();
        let deck = deck_dir.join("talk.pin");
        std::fs::write(&deck, b"--\n").unwrap();

        assert_eq!(
            resolve_local(Some(&deck), "linked.png", Access::Confined),
            None
        );
        assert_eq!(
            resolve_local(Some(&deck), "linked.png", Access::Compatible),
            Some(deck_dir.join("linked.png"))
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
