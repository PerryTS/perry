//! Shell out to the chosen installer with `--ignore-scripts` so no
//! package code executes during the install proper. When Socket Firewall
//! is available (`PERRY_SFW`, or `sfw` on PATH — see `firewall`) the
//! installer runs THROUGH it, adding network-time malware scanning to the
//! existing install-time scan + script gate.

use anyhow::{bail, Result};
use std::process::Command;

use super::detect::Installer;
use super::firewall::{self, FirewallStatus};
use super::InstallArgs;

/// Build and run the underlying installer command. Inherits stdio so
/// the user sees the installer's native progress output in real time.
///
/// Returns what happened to the network-time layer so the caller can put it
/// in the install report — an unfirewalled install must leave a record on
/// every path, not only the one where a human is reading stderr.
pub fn install(installer: &Installer, args: &InstallArgs) -> Result<FirewallStatus> {
    let sfw = if args.no_firewall {
        None
    } else {
        firewall::resolve_sfw()
    };
    let firewall_status = match (&sfw, args.no_firewall) {
        (Some(sfw_bin), _) => FirewallStatus::Active {
            sfw: sfw_bin.display().to_string(),
        },
        (None, true) => FirewallStatus::OptedOut,
        (None, false) => FirewallStatus::Unavailable,
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
    match &firewall_status {
        // Informational; suppressed in --json so a scripted run's stderr
        // stays quiet when everything is as it should be.
        FirewallStatus::Active { sfw } if !args.json => {
            eprintln!("perry install: network firewalled via {}", sfw)
        }
        // Fail-open must not be silent-open (mirrors the sfw shims), and
        // that applies to CI most of all. Printed regardless of --json: this
        // is stderr, so it cannot corrupt the JSON report on stdout, and the
        // run that most needs the warning is the unattended one.
        FirewallStatus::Unavailable => {
            let rack = firewall::rack_sfw_path(
                std::env::var("XDG_DATA_HOME").ok().as_deref(),
                dirs::home_dir().as_deref(),
            );
            eprintln!("{}", firewall::unavailable_notice(rack.as_deref()));
        }
        _ => {}
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

    let exit_status = cmd.status().map_err(|e| {
        anyhow::anyhow!(
            "failed to spawn `{} install`: {}. Is `{}` on PATH?",
            installer.binary(),
            e,
            installer.binary()
        )
    })?;

    if !exit_status.success() {
        bail!(
            "`{} install --ignore-scripts` exited with status {}",
            installer.binary(),
            exit_status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".into())
        );
    }

    Ok(firewall_status)
}
