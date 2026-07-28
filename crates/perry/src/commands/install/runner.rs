//! Shell out to the chosen installer with `--ignore-scripts` so no
//! package code executes during the install proper. When Socket Firewall
//! is available (perry's dev-tools rack, or PATH) the installer runs
//! THROUGH it, adding network-time malware scanning to the existing
//! install-time scan + script gate.

use anyhow::{bail, Result};
use std::process::Command;

use super::detect::Installer;
use super::firewall;
use super::InstallArgs;

/// Build and run the underlying installer command. Inherits stdio so
/// the user sees the installer's native progress output in real time.
pub fn install(installer: &Installer, args: &InstallArgs) -> Result<()> {
    let sfw = if args.no_firewall {
        None
    } else {
        firewall::resolve_sfw()
    };

    let mut cmd = match &sfw {
        Some(sfw_bin) => {
            // `sfw <installer> install ...` — sfw starts its scanning
            // proxy, points the child's proxy env at it, and forwards
            // stdio + exit status.
            let mut c = Command::new(sfw_bin);
            c.arg(installer.binary());
            for (k, v) in firewall::firewall_env() {
                c.env(k, v);
            }
            c
        }
        None => Command::new(installer.binary()),
    };
    if !args.json {
        match &sfw {
            Some(sfw_bin) => eprintln!(
                "perry install: network firewalled via {}",
                sfw_bin.display()
            ),
            // Fail-open must not be silent-open (mirrors the sfw shims).
            None if !args.no_firewall => eprintln!(
                "perry install: sfw not found — downloads run unfirewalled \
                 (install it via the repo's `tools:install`, or pass --no-firewall to silence this)"
            ),
            None => {}
        }
    }
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
