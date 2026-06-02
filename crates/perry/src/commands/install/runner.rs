//! Shell out to the chosen installer with `--ignore-scripts` so no
//! package code executes during the install proper.

use anyhow::{bail, Result};
use std::process::Command;

use super::detect::Installer;
use super::InstallArgs;

/// Build and run the underlying installer command. Inherits stdio so
/// the user sees the installer's native progress output in real time.
pub fn install(installer: &Installer, args: &InstallArgs) -> Result<()> {
    let mut cmd = Command::new(installer.binary());
    cmd.arg("install").arg("--ignore-scripts");

    // Translate Perry-side flags into the installer's native flag.
    match installer {
        Installer::Bun => {
            if args.save_dev {
                cmd.arg("--dev");
            }
            if args.global {
                cmd.arg("--global");
            }
            if args.production {
                cmd.arg("--production");
            }
        }
        Installer::Npm => {
            if args.save_dev {
                cmd.arg("--save-dev");
            }
            if args.global {
                cmd.arg("--global");
            }
            if args.production {
                // Modern npm prefers --omit=dev; --production is the legacy
                // spelling and still works on every version since npm 1.
                cmd.arg("--omit=dev");
            }
        }
    }

    for pkg in &args.packages {
        cmd.arg(pkg);
    }

    let status = cmd.status().map_err(|e| {
        anyhow::anyhow!(
            "failed to spawn `{} install`: {}. Is `{}` on PATH?",
            installer.binary(),
            e,
            installer.binary()
        )
    })?;

    if !status.success() {
        bail!(
            "`{} install --ignore-scripts` exited with status {}",
            installer.binary(),
            status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".into())
        );
    }

    Ok(())
}
