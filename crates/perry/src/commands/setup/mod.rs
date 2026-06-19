//! `perry setup` — guided credential setup wizard for App Store / Google Play distribution
//! (and toolchain setup for the "lightweight" Windows target — LLVM + xwin'd SDK).
//!
//! Split from a single 3,145-line file into per-platform sub-modules in
//! v0.5.1020. mod.rs is a re-export hub; the public API (`SetupArgs`,
//! `run`) stays here so `commands/mod.rs` keeps importing as before.

use anyhow::Result;
use clap::Args;
use console::style;
use std::path::PathBuf;
use std::process::Command;

mod android;
mod common_apple;
mod harmonyos;
mod helpers;
mod ios;
mod macos;
mod tvos;
mod visionos;
mod watchos;
mod windows;

// Per-platform wizards — used by `run` below.
pub(crate) use android::android_wizard;
pub(crate) use harmonyos::harmonyos_wizard;
pub(crate) use ios::{ios_development_setup, ios_wizard};
pub(crate) use macos::macos_wizard;
pub(crate) use tvos::tvos_wizard;
pub(crate) use visionos::visionos_wizard;
pub(crate) use watchos::watchos_wizard;
pub(crate) use windows::windows_wizard;

// Cross-platform helpers shared across wizards.
pub(crate) use common_apple::{generate_asc_jwt, prompt_api_credentials};
pub(crate) use helpers::{
    expand_tilde, press_enter_to_continue, prompt_file_path, prompt_output_path,
    update_perry_toml_android, update_perry_toml_encryption_exempt, update_perry_toml_ios,
    update_perry_toml_macos, update_perry_toml_section_bool,
};

// Cert utilities used by macos (re-exported in case any other module wants them).

// Per-platform per-bundle-id savers.

use super::publish::{is_interactive, load_config, save_config};

#[derive(Args, Debug)]
pub struct SetupArgs {
    /// Platform to configure: android, ios, visionos, macos, tvos, watchos, windows, harmonyos
    pub platform: Option<String>,

    /// (windows only) Accept the Microsoft Visual Studio Build Tools redistributable
    /// license required to download CRT + Windows SDK via xwin. Equivalent to
    /// answering "yes" at the interactive prompt; enables non-interactive / CI use.
    #[arg(long)]
    pub accept_license: bool,

    /// (ios only) Provision the currently-connected device for on-device
    /// *development* instead of App Store distribution: register the device's
    /// UDID and mint an "iOS App Development" provisioning profile via the App
    /// Store Connect API (saved to ~/.perry/<bundle>_dev.mobileprovision).
    /// Requires a prior `perry setup ios` to store API credentials.
    #[arg(long)]
    pub development: bool,
}

pub fn run(args: SetupArgs) -> Result<()> {
    let platform = match args.platform {
        Some(p) => p.to_lowercase(),
        None => {
            // No platform specified — prompt the user.
            let platforms = vec![
                "android",
                "ios",
                "macos",
                "visionos",
                "watchos",
                "tvos",
                "windows",
                "harmonyos",
            ];
            if !is_interactive() {
                eprintln!(
                    "{} no platform specified and running non-interactively",
                    style("✗").red().bold()
                );
                eprintln!("  Use `perry setup <platform>` (e.g. `perry setup ios`).");
                std::process::exit(2);
            }
            let idx = dialoguer::Select::new()
                .with_prompt("Which platform are you setting up?")
                .items(&platforms)
                .default(0)
                .interact()?;
            platforms[idx].to_string()
        }
    };

    if args.development && platform != "ios" {
        anyhow::bail!("--development is only supported for `perry setup ios`");
    }

    let mut saved = load_config();

    match platform.as_str() {
        "windows" => windows_wizard(args.accept_license)?,
        "android" => android_wizard(&mut saved)?,
        "ios" if args.development => {
            // Development-provisioning path: no global-config mutation, so
            // return before the save_config below.
            ios_development_setup(&saved)?;
            return Ok(());
        }
        "ios" => ios_wizard(&mut saved)?,
        "macos" => macos_wizard(&mut saved)?,
        "visionos" => visionos_wizard(&mut saved)?,
        "watchos" => watchos_wizard(&mut saved)?,
        "tvos" => tvos_wizard(&mut saved)?,
        "harmonyos" => harmonyos_wizard(&mut saved)?,
        other => {
            anyhow::bail!(
                "Unknown platform '{}'. Supported: android, ios, visionos, macos, tvos, watchos, windows, harmonyos",
                other
            );
        }
    }

    save_config(&saved)?;
    Ok(())
}

/// Locate `xwin.exe` on PATH (Windows wizard helper). Returns `None` if
/// not installed; the wizard then offers to install it via `cargo install xwin`.
///
/// `which` is not a built-in on Windows shells, so the wizard always failed
/// to find an already-installed `xwin.exe` (#1821). Use `where.exe` on
/// Windows — its output is one absolute path per matching extension on
/// PATH, so take the first line.
pub(crate) fn find_xwin_exe() -> Option<PathBuf> {
    let lookup = if cfg!(windows) { "where" } else { "which" };
    let out = Command::new(lookup).arg("xwin").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let first = stdout.lines().next()?.trim();
    if first.is_empty() {
        None
    } else {
        Some(PathBuf::from(first))
    }
}
