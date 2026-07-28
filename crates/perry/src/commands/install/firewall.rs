//! Socket Firewall (sfw) integration for the install wrapper.
//!
//! `perry install` already guarantees nothing *executes* at install time
//! (`--ignore-scripts` + offline scan + script allowlist); sfw adds the
//! network-time layer — a local proxy that scans registry traffic while
//! packages download, blocking known-malicious artifacts before they ever
//! reach `node_modules/`. The two compose: sfw gates the wire, the scanner
//! gates the tree, the allowlist gates execution.
//!
//! Resolution order: the perry dev-tools rack (where the supply-chain
//! tooling's `npm run tools:install` places an SRI-verified, exact-pinned
//! binary — see `external-tools.json` at the repo root), then `sfw` on
//! PATH. Fail-open when absent — a missing firewall must not break
//! installs — but never SILENT-open: one stderr note says so.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Well-known rack handle written by `external-tools.mts --install sfw`:
/// `$XDG_DATA_HOME/perry/dev-tools/bin/sfw` (XDG default `~/.local/share`).
/// Pure so it stays unit-testable; the caller decides existence.
pub fn rack_sfw_path(xdg_data_home: Option<&str>, home: Option<&Path>) -> Option<PathBuf> {
    let data_dir = match xdg_data_home {
        Some(x) if !x.is_empty() => PathBuf::from(x),
        _ => home?.join(".local").join("share"),
    };
    Some(
        data_dir
            .join("perry")
            .join("dev-tools")
            .join("bin")
            .join("sfw"),
    )
}

/// Probe whether a binary at `path` (or a bare name resolved via PATH)
/// responds to `--version`.
fn probe(bin: &Path) -> bool {
    Command::new(bin)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Resolve a usable sfw binary: rack first (pinned + SRI-verified beats
/// whatever PATH happens to hold), then PATH.
pub fn resolve_sfw() -> Option<PathBuf> {
    let xdg = env::var("XDG_DATA_HOME").ok();
    if let Some(rack) = rack_sfw_path(xdg.as_deref(), dirs::home_dir().as_deref()) {
        if rack.is_file() && probe(&rack) {
            return Some(rack);
        }
    }
    let bare = PathBuf::from("sfw");
    if probe(&bare) {
        return Some(bare);
    }
    None
}

/// Env the wrapped installer runs under when sfw fronts it.
///
/// The shim sentinels matter when the PM that PATH resolves is itself an
/// sfw shim (dev machines after `tools:install`): without them the shim
/// would wrap the already-wrapped invocation in a second nested proxy.
/// Setting them makes the shim exec the real binary — exactly the
/// re-entry case the sentinels exist for. `SFW_UNKNOWN_HOST_ACTION`
/// mirrors the shims: enterprise sfw defaults to BLOCK for non-registry
/// hosts, which breaks ordinary flows; free tier hardcodes ignore and
/// disregards the var, so setting it is always safe.
pub fn firewall_env() -> [(&'static str, &'static str); 4] {
    [
        ("SFW_SHIM_ACTIVE_NPM", "1"),
        ("SFW_SHIM_ACTIVE_BUN", "1"),
        ("SFW_SHIM_ACTIVE_YARN", "1"),
        ("SFW_UNKNOWN_HOST_ACTION", "ignore"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rack_path_honors_xdg_data_home() {
        let p = rack_sfw_path(Some("/custom/xdg"), Some(Path::new("/home/u"))).unwrap();
        assert_eq!(p, Path::new("/custom/xdg/perry/dev-tools/bin/sfw"));
    }

    #[test]
    fn rack_path_falls_back_to_home_local_share() {
        let p = rack_sfw_path(None, Some(Path::new("/home/u"))).unwrap();
        assert_eq!(p, Path::new("/home/u/.local/share/perry/dev-tools/bin/sfw"));
        // Empty XDG_DATA_HOME is "unset" per the basedir spec.
        let p = rack_sfw_path(Some(""), Some(Path::new("/home/u"))).unwrap();
        assert_eq!(p, Path::new("/home/u/.local/share/perry/dev-tools/bin/sfw"));
    }

    #[test]
    fn rack_path_requires_some_anchor() {
        assert!(rack_sfw_path(None, None).is_none());
    }

    #[test]
    fn firewall_env_covers_every_wrappable_installer_sentinel() {
        let env = firewall_env();
        // Both installers perry can pick must have their shim sentinel set,
        // or a shimmed PM would nest a second proxy inside this one.
        for needed in ["SFW_SHIM_ACTIVE_NPM", "SFW_SHIM_ACTIVE_BUN"] {
            assert!(env.iter().any(|(k, v)| *k == needed && *v == "1"));
        }
        assert!(env
            .iter()
            .any(|(k, v)| *k == "SFW_UNKNOWN_HOST_ACTION" && *v == "ignore"));
    }
}
