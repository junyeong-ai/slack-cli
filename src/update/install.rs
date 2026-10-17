use std::fs::File;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

const OIDC_ISSUER: &str = "https://token.actions.githubusercontent.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signature {
    /// `cosign` verified the release signature against the workflow identity.
    Verified,
    /// `cosign` is not installed, so the download rests on its checksum alone
    /// — the same trust `scripts/install.sh` falls back to.
    Unverified,
}

pub fn verify_checksum(file: &Path, checksum_file: &Path) -> Result<()> {
    let published = std::fs::read_to_string(checksum_file)
        .with_context(|| format!("could not read {}", checksum_file.display()))?;
    let expected = published
        .split_whitespace()
        .next()
        .context("the published checksum file was empty")?
        .to_ascii_lowercase();

    let bytes =
        std::fs::read(file).with_context(|| format!("could not read {}", file.display()))?;
    let actual = hex(&Sha256::digest(&bytes));

    if actual != expected {
        bail!(
            "checksum mismatch for {}: published {expected}, downloaded {actual}",
            file.display()
        );
    }
    Ok(())
}

/// Only the absence of `cosign` may lower the bar. A release that publishes no
/// bundle is refused rather than accepted on its checksum, which whoever
/// published the binary also controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignaturePolicy {
    Verify,
    Refuse,
    SkipUnverified,
}

pub fn signature_policy(cosign_installed: bool, bundle_published: bool) -> SignaturePolicy {
    match (cosign_installed, bundle_published) {
        (true, true) => SignaturePolicy::Verify,
        (true, false) => SignaturePolicy::Refuse,
        (false, _) => SignaturePolicy::SkipUnverified,
    }
}

pub fn cosign_installed() -> bool {
    Command::new("cosign").arg("version").output().is_ok()
}

/// Callers establish that `cosign` is present before reaching this.
pub fn verify_signature(file: &Path, bundle: &Path, repository: &str, version: &str) -> Result<()> {
    let identity = certificate_identity(repository, version);
    let output = Command::new("cosign")
        .arg("verify-blob")
        .arg("--bundle")
        .arg(bundle)
        .arg("--certificate-identity-regexp")
        .arg(&identity)
        .arg("--certificate-oidc-issuer")
        .arg(OIDC_ISSUER)
        .arg(file)
        .output()
        .context("could not run cosign")?;

    if !output.status.success() {
        bail!(
            "signature verification failed for {}: {}",
            file.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            use std::fmt::Write;
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// Pinning the tag, not just the workflow, refuses a genuinely-signed binary
/// lifted from another release and re-uploaded under this one's asset names.
fn certificate_identity(repository: &str, version: &str) -> String {
    format!(
        r"^https://github\.com/{}/\.github/workflows/release\.yml@refs/tags/v{}$",
        regex_escape(repository),
        regex_escape(version)
    )
}

fn regex_escape(value: &str) -> String {
    value.replace('.', r"\.").replace('+', r"\+")
}

/// Replaces the running executable.
///
/// The new file is staged in the destination directory so the final step is a
/// rename within one filesystem, which either happens or does not — a
/// half-written binary is never left in place.
pub fn replace(new_binary: &Path, destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .with_context(|| format!("{} has no parent directory", destination.display()))?;
    let file_name = destination
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("slack-cli");

    let mut staged = tempfile::Builder::new()
        .prefix(&format!(".{file_name}.update."))
        .tempfile_in(parent)
        .with_context(|| format!("could not create an update file in {}", parent.display()))?;
    let mut source = File::open(new_binary)
        .with_context(|| format!("could not open {}", new_binary.display()))?;
    std::io::copy(&mut source, staged.as_file_mut()).with_context(|| {
        format!(
            "could not stage the new binary at {}",
            staged.path().display()
        )
    })?;
    make_executable(staged.as_file())?;
    staged
        .as_file()
        .sync_all()
        .context("could not sync the staged binary")?;
    let staged = staged.into_temp_path();
    swap(staged, destination)?;
    Ok(())
}

#[cfg(unix)]
fn swap(staged: tempfile::TempPath, destination: &Path) -> Result<()> {
    // A running process holds its executable by inode, so renaming over it is
    // safe: this process keeps running from the old file until it exits.
    staged
        .persist(destination)
        .map_err(|error| error.error)
        .with_context(|| {
            format!(
                "could not replace {} (is it writable?)",
                destination.display()
            )
        })
}

#[cfg(windows)]
fn swap(staged: tempfile::TempPath, destination: &Path) -> Result<()> {
    // Windows will not replace a running executable, so the running one moves
    // aside first. It stays until a later update clears it: this process still
    // has it open.
    let displaced = destination.with_extension("old");
    let _ = std::fs::remove_file(&displaced);
    std::fs::rename(destination, &displaced)
        .with_context(|| format!("could not move {} aside", destination.display()))?;

    let Err(error) = staged.persist(destination).map_err(|error| error.error) else {
        return Ok(());
    };
    match std::fs::rename(&displaced, destination) {
        Ok(()) => Err(error)
            .with_context(|| format!("could not put the new binary at {}", destination.display())),
        Err(restore) => Err(error).with_context(|| {
            format!(
                "could not put the new binary at {}, and the previous one could not be moved \
                 back from {}: {restore}",
                destination.display(),
                displaced.display()
            )
        }),
    }
}

#[cfg(unix)]
fn make_executable(file: &File) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(std::fs::Permissions::from_mode(0o755))
        .context("could not mark the staged binary executable")
}

#[cfg(not(unix))]
fn make_executable(_: &File) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn a_matching_checksum_passes() {
        let dir = tempfile::tempdir().unwrap();
        let file = write(dir.path(), "slack-cli", "payload");
        let digest = hex(&Sha256::digest(b"payload"));
        let checksum = write(
            dir.path(),
            "slack-cli.sha256",
            &format!("{digest}  slack-cli\n"),
        );
        verify_checksum(&file, &checksum).unwrap();
    }

    #[test]
    fn a_mismatched_checksum_names_both_digests() {
        let dir = tempfile::tempdir().unwrap();
        let file = write(dir.path(), "slack-cli", "payload");
        let checksum = write(dir.path(), "slack-cli.sha256", "00  slack-cli\n");
        let err = verify_checksum(&file, &checksum).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("checksum mismatch"), "{message}");
        assert!(message.contains("published 00"), "{message}");
    }

    #[test]
    fn an_empty_checksum_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let file = write(dir.path(), "slack-cli", "payload");
        let checksum = write(dir.path(), "slack-cli.sha256", "   \n");
        assert!(verify_checksum(&file, &checksum).is_err());
    }

    #[test]
    fn only_a_missing_cosign_may_lower_the_bar() {
        assert_eq!(signature_policy(true, true), SignaturePolicy::Verify);
        assert_eq!(signature_policy(true, false), SignaturePolicy::Refuse);
        assert_eq!(
            signature_policy(false, true),
            SignaturePolicy::SkipUnverified
        );
        assert_eq!(
            signature_policy(false, false),
            SignaturePolicy::SkipUnverified
        );
    }

    #[test]
    fn the_certificate_identity_pins_one_workflow_and_one_tag() {
        assert_eq!(
            certificate_identity("junyeong-ai/slack-cli", "0.9.0"),
            r"^https://github\.com/junyeong-ai/slack-cli/\.github/workflows/release\.yml@refs/tags/v0\.9\.0$"
        );
    }

    #[test]
    fn the_identity_pattern_escapes_regex_syntax() {
        assert_eq!(regex_escape("owner/repo.rs"), r"owner/repo\.rs");
        assert_eq!(
            regex_escape("junyeong-ai/slack-cli"),
            "junyeong-ai/slack-cli"
        );
        assert_eq!(regex_escape("1.0.0-rc.1+build"), r"1\.0\.0-rc\.1\+build");
    }

    #[test]
    fn replacing_a_binary_swaps_its_contents_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let destination = write(dir.path(), "slack-cli", "old");
        let fresh = write(dir.path(), "downloaded", "new");

        replace(&fresh, &destination).unwrap();

        assert_eq!(std::fs::read_to_string(&destination).unwrap(), "new");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with('.'))
            .collect();
        assert!(
            leftovers.is_empty(),
            "staging file left behind: {leftovers:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn replacement_preserves_existing_staging_links_and_their_targets() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        for symbolic in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let destination = write(dir.path(), "slack-cli", "old");
            let fresh = write(dir.path(), "downloaded", "new");
            let unrelated = write(dir.path(), "unrelated", "private data");
            std::fs::set_permissions(&unrelated, std::fs::Permissions::from_mode(0o600)).unwrap();
            let existing = dir
                .path()
                .join(format!(".slack-cli.update.{}", std::process::id()));
            if symbolic {
                symlink(&unrelated, &existing).unwrap();
            } else {
                std::fs::hard_link(&unrelated, &existing).unwrap();
            }
            replace(&fresh, &destination).unwrap();
            assert_eq!(std::fs::read_to_string(&unrelated).unwrap(), "private data");
            assert_eq!(
                std::fs::metadata(&unrelated).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(std::fs::read_to_string(&existing).unwrap(), "private data");
            assert_eq!(std::fs::read_to_string(&destination).unwrap(), "new");
            assert!(
                !std::fs::symlink_metadata(&destination)
                    .unwrap()
                    .is_symlink()
            );
        }
    }

    #[test]
    fn failed_replacement_preserves_preexisting_staging_files() {
        let dir = tempfile::tempdir().unwrap();
        let destination = write(dir.path(), "slack-cli", "old");
        let existing = write(
            dir.path(),
            &format!(".slack-cli.update.{}", std::process::id()),
            "keep",
        );
        assert!(replace(&dir.path().join("missing"), &destination).is_err());
        assert_eq!(std::fs::read_to_string(&destination).unwrap(), "old");
        assert_eq!(std::fs::read_to_string(&existing).unwrap(), "keep");
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_swap_removes_only_its_own_staging_file() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("slack-cli");
        std::fs::create_dir(&destination).unwrap();
        let retained = write(&destination, "keep", "unrelated");
        let fresh = write(dir.path(), "downloaded", "new");
        assert!(replace(&fresh, &destination).is_err());
        assert_eq!(std::fs::read_to_string(retained).unwrap(), "unrelated");
        assert!(!std::fs::read_dir(dir.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with('.')
        }));
    }

    #[cfg(unix)]
    #[test]
    fn the_replacement_is_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let destination = write(dir.path(), "slack-cli", "old");
        let fresh = write(dir.path(), "downloaded", "new");

        replace(&fresh, &destination).unwrap();

        let mode = std::fs::metadata(&destination)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "mode was {mode:o}");
    }

    #[test]
    fn a_failed_stage_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("slack-cli");
        std::fs::write(&destination, "old").unwrap();

        let missing = dir.path().join("does-not-exist");
        assert!(replace(&missing, &destination).is_err());
        assert_eq!(std::fs::read_to_string(&destination).unwrap(), "old");

        let leftovers = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().starts_with('.'));
        assert!(!leftovers, "staging file left behind");
    }
}
