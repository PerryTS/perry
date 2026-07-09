//! #675 — codesigning for watchOS `.app` / widget `.appex` bundles so the
//! App Group entitlement is actually honored at runtime.
//!
//! `bundle_ios.rs` gets its device signature from the iOS resign path
//! (`commands/run/resign.rs`) and needs no signing on the simulator (the iOS
//! sim shares one data-container root, so app groups resolve unsigned). The
//! watchOS bundlers have no such path, so this module provides the minimal
//! piece the App Group container needs:
//!
//! - **Simulator** — ad-hoc sign (`codesign -s -`) with an entitlements plist
//!   carrying `com.apple.security.application-groups`. `containermanagerd` on
//!   the watch sim only vends the shared App Group container to a bundle whose
//!   signature declares that entitlement; without it `NSUserDefaults(suiteName:)`
//!   silently falls back to a per-app plist and the app ↔ widget never see each
//!   other's writes.
//! - **Device** — real signing identity + a provisioning profile that includes
//!   the App Group, both read from `[watchos]` in perry.toml
//!   (`signing_identity` / `provisioning_profile`). Nothing is hardcoded; when
//!   the config is absent the bundle is left unsigned (prior behavior) with a
//!   note pointing at `perry setup watchos`.
//!
//! Only invoked when an `app.entitlements` was emitted (i.e. `[watchos]
//! app_group` is set), so plain watch apps keep building unsigned.

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::OutputFormat;

/// `[watchos]` signing material from perry.toml, used for on-device signing.
/// Both fields optional: a bare `[watchos] app_group` with no signing config
/// still ad-hoc-signs for the simulator but leaves device builds unsigned.
#[derive(Default)]
pub(super) struct WatchSigningConfig {
    pub signing_identity: Option<String>,
    pub provisioning_profile: Option<String>,
}

/// Read `[watchos] signing_identity` / `provisioning_profile` by walking up
/// from `input` for the nearest perry.toml (same 5-parent walk the other
/// bundlers use). Returns an all-`None` config when absent.
pub(super) fn read_watch_signing_config(input: &Path) -> WatchSigningConfig {
    (|| -> Option<WatchSigningConfig> {
        let mut dir = input.canonicalize().ok()?;
        for _ in 0..5 {
            dir = dir.parent()?.to_path_buf();
            let toml_path = dir.join("perry.toml");
            if !toml_path.exists() {
                continue;
            }
            let doc: toml::Table = fs::read_to_string(&toml_path).ok()?.parse().ok()?;
            let watchos = doc.get("watchos").and_then(|v| v.as_table())?;
            return Some(WatchSigningConfig {
                signing_identity: watchos
                    .get("signing_identity")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                provisioning_profile: watchos
                    .get("provisioning_profile")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            });
        }
        None
    })()
    .unwrap_or_default()
}

/// Codesign an Apple bundle (`.app` or `.appex`) with `entitlements_path`.
///
/// `is_sim` selects ad-hoc (`-s -`) vs. real-identity signing. For device
/// builds `cfg.provisioning_profile` (when present) is embedded as
/// `embedded.mobileprovision` before signing so codesign can validate the
/// requested entitlements against the profile's capabilities.
///
/// Returns `Ok(true)` when the bundle was signed, `Ok(false)` when signing was
/// deliberately skipped (device build without `[watchos] signing_identity`),
/// and `Err` only when a signing attempt actually failed. A skip is not fatal:
/// the caller keeps the unsigned bundle plus install instructions.
pub(super) fn codesign_apple_bundle(
    bundle: &Path,
    entitlements_path: &Path,
    is_sim: bool,
    cfg: &WatchSigningConfig,
    format: OutputFormat,
) -> anyhow::Result<bool> {
    // Ad-hoc identity for the simulator; the real identity for device.
    let identity: String = if is_sim {
        "-".to_string()
    } else {
        match cfg.signing_identity.as_deref() {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => {
                if let OutputFormat::Text = format {
                    println!(
                        "  App Group entitlement written but bundle left unsigned — add \
                         `[watchos] signing_identity` + `provisioning_profile` to perry.toml \
                         (e.g. `perry setup watchos`) for on-device App Group access."
                    );
                }
                return Ok(false);
            }
        }
    };

    // Device: embed the provisioning profile so codesign can validate the
    // app-group entitlement against the profile's declared capabilities. A
    // device bundle with App Group entitlements is unusable without it, so
    // fail loudly rather than emit a "Codesigned" bundle that can't access
    // the group on-device.
    if !is_sim {
        let profile = cfg
            .provisioning_profile
            .as_deref()
            .filter(|p| !p.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "`[watchos] provisioning_profile` is required to sign a device bundle with \
                     App Group entitlements (e.g. `perry setup watchos`)"
                )
            })?;
        let bytes = fs::read(profile)
            .map_err(|e| anyhow::anyhow!("could not read provisioning_profile {profile}: {e}"))?;
        fs::write(bundle.join("embedded.mobileprovision"), bytes)?;
    }

    // Re-sign cleanly: drop any prior signature so codesign doesn't complain
    // about an already-sealed bundle (matches the iOS resign path).
    let _ = fs::remove_dir_all(bundle.join("_CodeSignature"));

    let status = Command::new("codesign")
        .args(["--force", "--sign", &identity, "--entitlements"])
        .arg(entitlements_path)
        .arg("--generate-entitlement-der")
        .arg(bundle)
        .status()
        .map_err(|e| anyhow::anyhow!("failed to invoke codesign: {e}"))?;
    if !status.success() {
        anyhow::bail!(
            "codesign failed for {} (exit {})",
            bundle.display(),
            status.code().unwrap_or(-1)
        );
    }

    if let OutputFormat::Text = format {
        let how = if is_sim {
            "ad-hoc".to_string()
        } else {
            format!("identity {identity}")
        };
        println!("  Codesigned {} ({how}).", bundle.display());
    }
    Ok(true)
}
