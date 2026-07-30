//! Socket Firewall (sfw) integration for the install wrapper.
//!
//! `perry install` already guarantees nothing *executes* at install time
//! (`--ignore-scripts` + offline scan + script allowlist); sfw adds the
//! network-time layer — a local proxy that scans registry traffic while
//! packages download, blocking known-malicious artifacts before they ever
//! reach `node_modules/`. The two compose: sfw gates the wire, the scanner
//! gates the tree, the allowlist gates execution.
//!
//! ## Resolution order, and why it is not "$HOME first"
//!
//! `PERRY_SFW` (an explicit path), then `sfw` found by walking `PATH`.
//! That is the whole list.
//!
//! It used to prefer `$XDG_DATA_HOME/perry/dev-tools/bin/sfw` — the perry
//! dev-tools rack — over PATH, gated only on `is_file()` plus a successful
//! `--version`. That was wrong in a way worth spelling out, because the
//! comment here used to claim the opposite:
//!
//! - Nothing in the shipped perry binary verifies that file. The SRI pins in
//!   `external-tools.json` are checked by the repo's `tools:install` script,
//!   in a contributor's checkout, at a completely different time. An end
//!   user's `perry install` has no manifest, no hash, and no way to get one.
//! - The perry binary never creates that directory either, so its existence
//!   is not evidence that perry tooling put it there.
//! - Anything able to write under `$HOME` — an earlier malicious
//!   `postinstall`, a shared CI home cache, a relative `XDG_DATA_HOME` —
//!   therefore chose the binary perry executes as the user, on every
//!   install, while perry printed `network firewalled via <that path>`.
//!   The security claim was doing the attacker's marketing.
//!
//! An ownership/permission check would not have fixed it: the threat is code
//! already running AS the user, which can write a user-owned 0644 file. The
//! only honest options were "verify against a pin" (no pin is available at
//! runtime) or "stop preferring an unverified path". This takes the second.
//!
//! `PERRY_SFW` keeps the dev-tools workflow available — it is an explicit
//! act of trust by whoever sets it, the same trust model as PATH — and the
//! not-found message names the rack path so contributors know what to set.
//!
//! PATH is walked here rather than handed to `Command::new("sfw")`, because
//! bare-name spawning consults the current directory: on Windows the
//! `CreateProcess` search order includes it, and on POSIX an empty `PATH`
//! entry means `.`. `perry install` runs in a project directory whose
//! contents are, by definition, not yet scanned.
//!
//! Fail-open when absent — a missing firewall must not break installs — but
//! never SILENT-open: the notice goes to stderr on every run (including
//! `--json`, where stdout carries the report) and the outcome is recorded in
//! the install report.

use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::detect::Installer;

/// Environment variable naming an sfw binary explicitly.
pub const SFW_PATH_ENV: &str = "PERRY_SFW";

/// Well-known rack handle written by `external-tools.mts --install sfw`:
/// `$XDG_DATA_HOME/perry/dev-tools/bin/sfw` (XDG default `~/.local/share`).
///
/// This is NOT probed automatically — see the module docs. It is used only to
/// tell a contributor what to point `PERRY_SFW` at. Pure so it stays
/// unit-testable; the caller decides existence.
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

/// Candidate absolute paths for `name` derived from a `PATH` value, in order.
///
/// Pure (existence is the caller's job) so the two traps this exists to avoid
/// are unit-testable:
///
/// - An EMPTY `PATH` entry means the current directory to `execvp`. Dropped.
/// - A RELATIVE `PATH` entry resolves against the current directory. Dropped.
///
/// `pathext` supplies the Windows executable suffixes (`.EXE;.CMD;…`); pass
/// `None` on Unix, where the name is used as-is.
pub fn path_candidates(
    name: &str,
    path_var: Option<&OsStr>,
    pathext: Option<&OsStr>,
) -> Vec<PathBuf> {
    let Some(path_var) = path_var else {
        return Vec::new();
    };
    let suffixes: Vec<String> = match pathext {
        Some(exts) => {
            let mut out = vec![String::new()];
            out.extend(
                exts.to_string_lossy()
                    .split(';')
                    .filter(|e| !e.is_empty())
                    .map(|e| e.to_string()),
            );
            out
        }
        None => vec![String::new()],
    };
    let mut out = Vec::new();
    for dir in env::split_paths(path_var) {
        // An empty entry is `.` and a relative entry is `./…`; the current
        // directory is exactly what must not decide which binary runs.
        if dir.as_os_str().is_empty() || !dir.is_absolute() {
            continue;
        }
        for suffix in &suffixes {
            out.push(dir.join(format!("{name}{suffix}")));
        }
    }
    out
}

/// Probe whether the binary at `path` responds to `--version`.
///
/// Always an absolute path by the time it gets here (see `path_candidates`
/// and `resolve_sfw`) so this never re-enters PATH/CWD resolution.
fn probe(bin: &Path) -> bool {
    Command::new(bin)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn explicit_sfw_path(value: std::ffi::OsString) -> Option<PathBuf> {
    if value.is_empty() {
        return None;
    }
    let path = PathBuf::from(value);
    path.is_absolute().then_some(path)
}

/// Resolve a usable sfw binary: `PERRY_SFW` if set, else the first `sfw` on
/// `PATH` that answers `--version`. Never consults `$HOME` or the current
/// directory — see the module docs.
pub fn resolve_sfw() -> Option<PathBuf> {
    if let Some(explicit) = env::var_os(SFW_PATH_ENV) {
        let explicit = explicit_sfw_path(explicit)?;
        // An explicit request is honored or reported, never quietly replaced
        // by a different binary: falling through to PATH here would mean the
        // user asked for one firewall and silently got another.
        return if explicit.is_file() && probe(&explicit) {
            Some(explicit)
        } else {
            None
        };
    }
    let pathext = if cfg!(windows) {
        env::var_os("PATHEXT")
    } else {
        None
    };
    for candidate in path_candidates("sfw", env::var_os("PATH").as_deref(), pathext.as_deref()) {
        if candidate.is_file() && probe(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// What happened to the network-time layer for one `perry install`.
///
/// Recorded in the install report so the `--json` path — CI — has the same
/// account of it that a human reading stderr gets. An unfirewalled install
/// that leaves no trace is the failure mode this exists to close.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum FirewallStatus {
    /// The installer ran through sfw at this path.
    Active { sfw: String },
    /// `--no-firewall` was passed; the user opted out.
    OptedOut,
    /// No sfw binary was found. The install ran UNFIREWALLED.
    ///
    /// Also the Default, deliberately: a report that failed to record what
    /// happened must claim LESS protection than it got, never more.
    #[default]
    Unavailable,
}

impl FirewallStatus {
    /// Whether network traffic was actually scanned. Only `Active` counts —
    /// `OptedOut` and `Unavailable` are both unfirewalled installs, they
    /// differ only in whether the user asked for it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_active(&self) -> bool {
        matches!(self, FirewallStatus::Active { .. })
    }
}

/// The stderr line for a fail-open, naming the rack path so a contributor who
/// ran `tools:install` knows what to point `PERRY_SFW` at.
pub fn unavailable_notice(rack_hint: Option<&Path>) -> String {
    let mut msg = String::from(
        "perry install: sfw not found — downloads run UNFIREWALLED \
         (pass --no-firewall to silence this)",
    );
    if let Some(rack) = rack_hint {
        msg.push_str(&format!(
            "\n  install it with the repo's `tools:install`, then export {}={}",
            SFW_PATH_ENV,
            rack.display()
        ));
    }
    msg
}

/// Env the wrapped installer runs under when sfw fronts it.
///
/// The shim sentinels matter when the PM that PATH resolves is itself an
/// sfw shim (dev machines after `tools:install`): without them the shim
/// would wrap the already-wrapped invocation in a second nested proxy.
/// Setting them makes the shim exec the real binary — exactly the
/// re-entry case the sentinels exist for.
///
/// Built by iterating `Installer::ALL` rather than from a literal list, so a
/// third installer variant cannot ship without its sentinel.
/// `SFW_UNKNOWN_HOST_ACTION` mirrors the shims: enterprise sfw defaults to
/// BLOCK for non-registry hosts, which breaks ordinary flows; free tier
/// hardcodes ignore and disregards the var, so setting it is always safe.
pub fn firewall_env() -> Vec<(&'static str, &'static str)> {
    let mut env: Vec<(&'static str, &'static str)> = Installer::ALL
        .iter()
        .map(|installer| (installer.shim_sentinel_env(), "1"))
        .collect();
    // yarn is not an installer perry can pick, but a yarn shim can still sit
    // on PATH and a package's own scripts may reach for it.
    env.push(("SFW_SHIM_ACTIVE_YARN", "1"));
    env.push(("SFW_UNKNOWN_HOST_ACTION", "ignore"));
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

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
    fn firewall_env_covers_every_installer_sentinel() {
        let env = firewall_env();
        // Iterate the enum, not a literal list: a third variant must not be
        // able to ship without a sentinel while this test still passes.
        for installer in Installer::ALL {
            let needed = installer.shim_sentinel_env();
            assert!(
                env.iter().any(|(k, v)| *k == needed && *v == "1"),
                "installer {:?} has no shim sentinel in firewall_env()",
                installer
            );
        }
        assert!(env
            .iter()
            .any(|(k, v)| *k == "SFW_UNKNOWN_HOST_ACTION" && *v == "ignore"));
    }

    #[test]
    fn installer_sentinels_are_distinct() {
        // A copy-paste that gave two variants the same sentinel would leave
        // one PM unprotected while the loop above still passed.
        let mut seen = std::collections::HashSet::new();
        for installer in Installer::ALL {
            assert!(
                seen.insert(installer.shim_sentinel_env()),
                "duplicate shim sentinel for {:?}",
                installer
            );
        }
    }

    fn joined(dirs: &[&str]) -> OsString {
        env::join_paths(dirs.iter().map(Path::new)).unwrap()
    }

    #[test]
    fn path_candidates_are_absolute_and_ordered() {
        let path = joined(&["/usr/local/bin", "/usr/bin"]);
        let got = path_candidates("sfw", Some(path.as_os_str()), None);
        assert_eq!(
            got,
            vec![
                PathBuf::from("/usr/local/bin/sfw"),
                PathBuf::from("/usr/bin/sfw"),
            ]
        );
    }

    #[test]
    fn path_candidates_drop_the_current_directory() {
        // An empty entry and a relative entry both resolve against CWD, which
        // is the project being installed — the one directory whose contents
        // must not choose the firewall binary.
        let path = OsString::from(":.:relative/bin:/usr/bin");
        let got = path_candidates("sfw", Some(path.as_os_str()), None);
        assert_eq!(got, vec![PathBuf::from("/usr/bin/sfw")]);
    }

    #[test]
    fn path_candidates_without_a_path_var_are_empty() {
        assert!(path_candidates("sfw", None, None).is_empty());
    }

    #[test]
    fn explicit_sfw_path_rejects_empty_and_relative_values() {
        assert!(explicit_sfw_path(OsString::new()).is_none());
        assert!(explicit_sfw_path(OsString::from("./sfw")).is_none());
        assert!(explicit_sfw_path(OsString::from("tools/sfw")).is_none());
        assert_eq!(
            explicit_sfw_path(OsString::from("/tools/bin/sfw")),
            Some(PathBuf::from("/tools/bin/sfw"))
        );
    }

    #[test]
    fn path_candidates_expand_windows_pathext() {
        let path = joined(&["/tools/bin"]);
        let pathext = OsString::from(".COM;.EXE");
        let got = path_candidates("sfw", Some(path.as_os_str()), Some(pathext.as_os_str()));
        assert_eq!(
            got,
            vec![
                PathBuf::from("/tools/bin/sfw"),
                PathBuf::from("/tools/bin/sfw.COM"),
                PathBuf::from("/tools/bin/sfw.EXE"),
            ]
        );
    }

    #[test]
    fn unavailable_notice_says_unfirewalled_and_names_the_rack() {
        let msg = unavailable_notice(Some(Path::new(
            "/home/u/.local/share/perry/dev-tools/bin/sfw",
        )));
        assert!(msg.contains("UNFIREWALLED"));
        assert!(msg.contains(SFW_PATH_ENV));
        assert!(msg.contains("/home/u/.local/share/perry/dev-tools/bin/sfw"));
    }

    #[test]
    fn firewall_status_only_active_counts_as_scanned() {
        assert!(FirewallStatus::Active {
            sfw: "/usr/bin/sfw".into()
        }
        .is_active());
        assert!(!FirewallStatus::OptedOut.is_active());
        assert!(!FirewallStatus::Unavailable.is_active());
    }

    #[test]
    fn firewall_status_defaults_to_unavailable() {
        // A code path that forgets to record the firewall must not end up
        // claiming the install was protected.
        assert_eq!(FirewallStatus::default(), FirewallStatus::Unavailable);
        assert!(!FirewallStatus::default().is_active());
    }

    #[test]
    fn firewall_status_serializes_with_a_state_tag() {
        let json = serde_json::to_string(&FirewallStatus::Unavailable).unwrap();
        assert_eq!(json, r#"{"state":"unavailable"}"#);
        let json = serde_json::to_string(&FirewallStatus::Active {
            sfw: "/usr/bin/sfw".into(),
        })
        .unwrap();
        assert_eq!(json, r#"{"state":"active","sfw":"/usr/bin/sfw"}"#);
    }
}
