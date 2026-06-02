//! Lifecycle-script execution. Only invoked after the scan passes
//! (or its findings have been overridden). Runs `preinstall`,
//! `install`, and `postinstall` for packages on a trust allowlist —
//! the bundled one in [`allowlist`] plus any names the user opted
//! into via `package.json -> perry.allowScripts` or the
//! `--run-scripts` / `--run-scripts-all` CLI flags.
//!
//! Scripts are executed under the host shell (`sh -c` on Unix,
//! `cmd /C` on Windows), in the package's directory, with PATH
//! prefixed so `node_modules/.bin` binaries (typescript, node-gyp,
//! prebuild-install, …) are reachable without absolute paths.
//!
//! v2 will run these in an OS sandbox with redacted env (no SSH /
//! AWS / GitHub / npm tokens) and a tight filesystem + network
//! profile. v1 trusts the allowlist: a compromised version of an
//! allowlisted package's script WILL run.

use anyhow::{bail, Result};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use super::allowlist;
use super::scanner::ScannedPackage;
use super::InstallArgs;

const LIFECYCLE_KEYS: &[&str] = &["preinstall", "install", "postinstall"];

/// Top-level entry: decide which packages are allowed to run scripts,
/// run them, and return a brief summary line per skipped package
/// that had scripts but didn't pass the trust gate.
pub fn run_all(
    packages: &[ScannedPackage],
    project_root: &Path,
    args: &InstallArgs,
) -> Result<RunSummary> {
    let user_opt_in = read_user_allowscripts(project_root);
    let mut summary = RunSummary::default();

    for pkg in packages {
        if !has_lifecycle_scripts(pkg) {
            continue;
        }

        let trusted = is_trusted(&pkg.name, &user_opt_in, args);
        if !trusted {
            summary.skipped.push(pkg.name.clone());
            continue;
        }

        run_package(pkg, project_root)?;
        summary.ran.push(pkg.name.clone());
    }
    Ok(summary)
}

#[derive(Default, Debug)]
pub struct RunSummary {
    pub ran: Vec<String>,
    pub skipped: Vec<String>,
}

fn has_lifecycle_scripts(pkg: &ScannedPackage) -> bool {
    let scripts = match pkg.manifest.get("scripts").and_then(|v| v.as_object()) {
        Some(s) => s,
        None => return false,
    };
    LIFECYCLE_KEYS
        .iter()
        .any(|k| scripts.get(*k).and_then(|v| v.as_str()).is_some())
}

fn is_trusted(name: &str, user_opt_in: &BTreeSet<String>, args: &InstallArgs) -> bool {
    if args.run_scripts_all {
        return true;
    }
    if allowlist::is_bundled(name) {
        return true;
    }
    if user_opt_in.contains(name) {
        return true;
    }
    if args.run_scripts.iter().any(|p| p == name) {
        return true;
    }
    false
}

/// Read `perry.allowScripts` from the project root's package.json.
/// Returns an empty set if the file or key is missing — this is the
/// non-Perry-project case, where we just rely on the bundled
/// allowlist + CLI flags.
fn read_user_allowscripts(project_root: &Path) -> BTreeSet<String> {
    let pkg_json = project_root.join("package.json");
    let content = match std::fs::read_to_string(&pkg_json) {
        Ok(c) => c,
        Err(_) => return BTreeSet::new(),
    };
    let json: Value = match serde_json::from_str(&content) {
        Ok(j) => j,
        Err(_) => return BTreeSet::new(),
    };
    json.get("perry")
        .and_then(|v| v.get("allowScripts"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn run_package(pkg: &ScannedPackage, project_root: &Path) -> Result<()> {
    let scripts = pkg
        .manifest
        .get("scripts")
        .and_then(|v| v.as_object())
        .expect("caller already verified has_lifecycle_scripts");

    let augmented_path = augment_path(project_root, &pkg.dir);

    for key in LIFECYCLE_KEYS {
        let body = match scripts.get(*key).and_then(|v| v.as_str()) {
            Some(b) => b.trim(),
            None => continue,
        };
        if body.is_empty() {
            continue;
        }

        let (shell, flag) = if cfg!(windows) {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };

        let status = Command::new(shell)
            .arg(flag)
            .arg(body)
            .current_dir(&pkg.dir)
            .env("PATH", &augmented_path)
            .env("npm_lifecycle_event", key)
            .env("npm_lifecycle_script", body)
            .env("npm_package_name", &pkg.name)
            .env("npm_package_version", &pkg.version)
            // INIT_CWD is what npm sets to the project root — many
            // scripts read it to locate sibling packages.
            .env("INIT_CWD", project_root)
            .status()
            .map_err(|e| {
                anyhow::anyhow!("failed to spawn shell for {} '{}': {}", pkg.name, key, e)
            })?;

        if !status.success() {
            bail!(
                "lifecycle script '{}' for package '{}' exited with status {}",
                key,
                pkg.name,
                status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".into())
            );
        }
    }
    Ok(())
}

fn augment_path(project_root: &Path, pkg_dir: &Path) -> std::ffi::OsString {
    let pkg_bin = pkg_dir.join("node_modules").join(".bin");
    let root_bin = project_root.join("node_modules").join(".bin");
    let sep = if cfg!(windows) { ";" } else { ":" };
    let mut out = std::ffi::OsString::new();
    if pkg_bin.is_dir() {
        out.push(pkg_bin.as_os_str());
        out.push(sep);
    }
    if root_bin.is_dir() {
        out.push(root_bin.as_os_str());
        out.push(sep);
    }
    if let Some(existing) = std::env::var_os("PATH") {
        out.push(existing);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn mk_pkg(name: &str, scripts: serde_json::Value) -> ScannedPackage {
        ScannedPackage {
            name: name.into(),
            version: "1.0.0".into(),
            dir: PathBuf::from("/t"),
            manifest: json!({"name": name, "version": "1.0.0", "scripts": scripts}),
        }
    }

    fn args() -> InstallArgs {
        InstallArgs {
            packages: vec![],
            save_dev: false,
            global: false,
            production: false,
            installer: None,
            skip_scan: false,
            allow_risky: vec![],
            allow_risky_all: false,
            run_scripts: vec![],
            run_scripts_all: false,
            check_freshness: false,
            json: false,
        }
    }

    #[test]
    fn has_lifecycle_detects_postinstall() {
        let p = mk_pkg("x", json!({"postinstall": "echo"}));
        assert!(has_lifecycle_scripts(&p));
    }

    #[test]
    fn has_lifecycle_ignores_non_install_scripts() {
        let p = mk_pkg("x", json!({"build": "tsc", "test": "jest"}));
        assert!(!has_lifecycle_scripts(&p));
    }

    #[test]
    fn trust_bundled() {
        assert!(is_trusted("esbuild", &BTreeSet::new(), &args()));
    }

    #[test]
    fn trust_user_opt_in() {
        let mut s = BTreeSet::new();
        s.insert("my-internal-pkg".to_string());
        assert!(is_trusted("my-internal-pkg", &s, &args()));
    }

    #[test]
    fn trust_run_scripts_cli_flag() {
        let mut a = args();
        a.run_scripts = vec!["mypkg".into()];
        assert!(is_trusted("mypkg", &BTreeSet::new(), &a));
    }

    #[test]
    fn trust_run_scripts_all_flag() {
        let mut a = args();
        a.run_scripts_all = true;
        assert!(is_trusted("random-untrusted", &BTreeSet::new(), &a));
    }

    #[test]
    fn untrusted_by_default() {
        assert!(!is_trusted("random", &BTreeSet::new(), &args()));
    }

    #[test]
    fn read_allowscripts_from_package_json() {
        let td = TempDir::new().unwrap();
        fs::write(
            td.path().join("package.json"),
            r#"{"name":"p","perry":{"allowScripts":["custom-pkg","another"]}}"#,
        )
        .unwrap();
        let set = read_user_allowscripts(td.path());
        assert!(set.contains("custom-pkg"));
        assert!(set.contains("another"));
    }

    #[test]
    fn read_allowscripts_missing_section_is_empty() {
        let td = TempDir::new().unwrap();
        fs::write(td.path().join("package.json"), r#"{"name":"p"}"#).unwrap();
        assert!(read_user_allowscripts(td.path()).is_empty());
    }

    #[test]
    fn read_allowscripts_missing_file_is_empty() {
        let td = TempDir::new().unwrap();
        assert!(read_user_allowscripts(td.path()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn run_lifecycle_executes_script() {
        // Real shell-out: write a tiny package, allowlist it via
        // run_scripts_all, and assert the postinstall ran (it touches
        // a sentinel file).
        let td = TempDir::new().unwrap();
        let pkg_dir = td.path().join("node_modules/sentinel-pkg");
        fs::create_dir_all(&pkg_dir).unwrap();
        let sentinel = pkg_dir.join("ran.txt");
        let manifest = json!({
            "name":"sentinel-pkg","version":"1.0.0",
            "scripts": {"postinstall": format!("touch {}", sentinel.display())}
        });
        fs::write(pkg_dir.join("package.json"), manifest.to_string()).unwrap();
        let pkg = ScannedPackage {
            name: "sentinel-pkg".into(),
            version: "1.0.0".into(),
            dir: pkg_dir,
            manifest,
        };
        let mut a = args();
        a.run_scripts_all = true;
        let summary = run_all(std::slice::from_ref(&pkg), td.path(), &a).unwrap();
        assert_eq!(summary.ran, vec!["sentinel-pkg".to_string()]);
        assert!(sentinel.exists(), "postinstall sentinel was not created");
    }

    #[cfg(unix)]
    #[test]
    fn run_lifecycle_skips_untrusted() {
        let td = TempDir::new().unwrap();
        let pkg_dir = td.path().join("node_modules/random-pkg");
        fs::create_dir_all(&pkg_dir).unwrap();
        let sentinel = pkg_dir.join("ran.txt");
        let manifest = json!({
            "name":"random-pkg","version":"1.0.0",
            "scripts": {"postinstall": format!("touch {}", sentinel.display())}
        });
        fs::write(pkg_dir.join("package.json"), manifest.to_string()).unwrap();
        let pkg = ScannedPackage {
            name: "random-pkg".into(),
            version: "1.0.0".into(),
            dir: pkg_dir,
            manifest,
        };
        let summary = run_all(std::slice::from_ref(&pkg), td.path(), &args()).unwrap();
        assert!(summary.ran.is_empty());
        assert_eq!(summary.skipped, vec!["random-pkg".to_string()]);
        assert!(
            !sentinel.exists(),
            "postinstall should NOT have run for untrusted pkg"
        );
    }
}
