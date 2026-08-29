pub mod install;
pub mod release;
pub mod target;

use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use install::{Signature, SignaturePolicy};
use release::Releases;

pub struct UpdateRequest {
    /// A specific release to install. Defaults to the latest.
    pub version: Option<String>,
    /// Report what an update would do without touching the binary.
    pub check: bool,
    /// Reinstall even when the running version already matches.
    pub force: bool,
    /// Skip the confirmation prompt.
    pub assume_yes: bool,
    /// Environment facts the update depends on. `None` means detect: the real
    /// GitHub API, the running executable, and whether `cosign` is on `PATH`.
    /// Tests supply them so an update never depends on the machine it runs on.
    pub api_base: Option<String>,
    pub binary: Option<PathBuf>,
    pub cosign: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Updated,
    Reinstalled,
    AlreadyCurrent,
    UpdateAvailable,
    Cancelled,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Updated => "updated",
            Self::Reinstalled => "reinstalled",
            Self::AlreadyCurrent => "already_current",
            Self::UpdateAvailable => "update_available",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug)]
pub struct UpdateOutcome {
    pub action: Action,
    pub from: String,
    pub to: String,
    pub target: &'static str,
    pub binary: PathBuf,
    pub signature: Option<Signature>,
}

pub async fn run(request: UpdateRequest) -> Result<UpdateOutcome> {
    let from = env!("CARGO_PKG_VERSION").to_string();
    let target = target::current().context(
        "this build's platform has no published release asset; \
         reinstall from source with cargo",
    )?;

    let repository = release::repository()?;
    let releases = match request.api_base {
        Some(base) => Releases::with_api_base(repository.clone(), base)?,
        None => Releases::new(repository.clone())?,
    };

    let release = match &request.version {
        Some(version) => releases.tagged(version).await?,
        None => releases.latest().await?,
    };
    let to = release.version().to_string();

    // The running executable may be reached through a symlink or a version
    // manager's shim; the rename has to land on the file itself.
    let binary = match request.binary {
        Some(path) => path,
        None => std::env::current_exe().context("could not determine the running binary's path")?,
    };
    let binary = binary.canonicalize().unwrap_or(binary);

    let unchanged = |action| UpdateOutcome {
        action,
        from: from.clone(),
        to: to.clone(),
        target,
        binary: binary.clone(),
        signature: None,
    };

    if request.check {
        return Ok(unchanged(if to == from {
            Action::AlreadyCurrent
        } else {
            Action::UpdateAvailable
        }));
    }

    if to == from && !request.force {
        return Ok(unchanged(Action::AlreadyCurrent));
    }

    if !confirm(
        &from,
        &to,
        request.assume_yes,
        std::io::stdin().is_terminal(),
    )? {
        return Ok(unchanged(Action::Cancelled));
    }

    let workspace = tempfile::tempdir().context("could not create a download directory")?;
    let asset_name = target::binary_asset(&to, target);

    let binary_asset = release.require(&asset_name)?;
    let checksum_asset = release.require(&format!("{asset_name}.sha256"))?;

    let downloaded = releases.download(binary_asset, workspace.path()).await?;
    let checksum = releases.download(checksum_asset, workspace.path()).await?;
    install::verify_checksum(&downloaded, &checksum)?;

    let bundle_asset = release.asset(&format!("{asset_name}.bundle"));
    let cosign = request.cosign.unwrap_or_else(install::cosign_installed);
    let signature = match install::signature_policy(cosign, bundle_asset.is_some()) {
        SignaturePolicy::Verify => {
            let asset = bundle_asset.expect("the policy saw a published bundle");
            let bundle = releases.download(asset, workspace.path()).await?;
            install::verify_signature(&downloaded, &bundle, &repository, &to)?;
            Signature::Verified
        }
        SignaturePolicy::Refuse => bail!(
            "release {} publishes no signature for {asset_name}. \
             cosign is installed, so this will not be installed on its checksum alone",
            release.tag_name
        ),
        SignaturePolicy::SkipUnverified => Signature::Unverified,
    };

    install::replace(&downloaded, &binary)?;

    Ok(UpdateOutcome {
        action: if to == from {
            Action::Reinstalled
        } else {
            Action::Updated
        },
        from,
        to,
        target,
        binary,
        signature: Some(signature),
    })
}

fn confirm(from: &str, to: &str, assume_yes: bool, interactive: bool) -> Result<bool> {
    if assume_yes {
        return Ok(true);
    }
    if !interactive {
        bail!("replacing the running binary needs confirmation. re-run with --yes");
    }
    print!("Replace the running binary (v{from} → v{to})? [y/N]: ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_names_are_stable() {
        for (action, name) in [
            (Action::Updated, "updated"),
            (Action::Reinstalled, "reinstalled"),
            (Action::AlreadyCurrent, "already_current"),
            (Action::UpdateAvailable, "update_available"),
            (Action::Cancelled, "cancelled"),
        ] {
            assert_eq!(action.as_str(), name);
        }
    }

    #[test]
    fn confirmation_is_refused_rather_than_assumed_off_a_terminal() {
        let err = confirm("0.8.0", "0.9.0", false, false).unwrap_err();
        assert!(err.to_string().contains("--yes"), "{err}");
    }

    #[test]
    fn assume_yes_needs_no_prompt() {
        assert!(confirm("0.8.0", "0.9.0", true, false).unwrap());
        assert!(confirm("0.8.0", "0.9.0", true, true).unwrap());
    }
}
