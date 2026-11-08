use super::{lifecycle, scanner, InstallArgs};
use clap::Parser;
use serde_json::json;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

#[derive(Parser)]
struct TestCli {
    #[command(flatten)]
    args: InstallArgs,
}

fn args(flags: &[&str]) -> InstallArgs {
    TestCli::parse_from(std::iter::once("perry-install").chain(flags.iter().copied())).args
}

fn write_package(root: &Path, installed: &str, declared: &str) {
    let dir = root.join("node_modules").join(installed);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.json"),
        json!({
            "name": declared,
            "version": "1.0.0",
            "scripts": {"postinstall": "printf approved > ran.txt"}
        })
        .to_string(),
    )
    .unwrap();
}

fn run(root: &Path, args: &InstallArgs) -> lifecycle::RunSummary {
    let packages = scanner::discover_packages(&root.join("node_modules"));
    let report = scanner::scan_packages(&packages, &[], false);
    assert!(matches!(report.verdict, scanner::report::Verdict::Clean));
    lifecycle::run_all(&packages, root, args).unwrap()
}

#[test]
fn lifecycle_requires_explicit_approval_even_for_bundled_names() {
    let _env = crate::test_env_lock::env_lock();
    let root = TempDir::new().unwrap();
    let names = ["esbuild", "lightningcss-unrelated", "@esbuild/unrelated"];
    for name in names {
        write_package(root.path(), name, name);
    }

    let summary = run(root.path(), &args(&[]));
    assert!(summary.ran.is_empty());
    let mut skipped = summary.skipped;
    skipped.sort();
    let mut expected = names.map(String::from);
    expected.sort();
    assert_eq!(skipped, expected);
    for name in names {
        assert!(!root
            .path()
            .join("node_modules")
            .join(name)
            .join("ran.txt")
            .exists());
    }
}

#[test]
fn lifecycle_approval_uses_installed_identity_not_manifest_name() {
    let _env = crate::test_env_lock::env_lock();
    let root = TempDir::new().unwrap();
    write_package(root.path(), "approved-fixture", "esbuild");
    write_package(root.path(), "untrusted-fixture", "esbuild");

    let summary = run(root.path(), &args(&["--run-scripts", "esbuild"]));
    assert!(summary.ran.is_empty());
    assert_eq!(summary.skipped.len(), 2);
    for name in ["approved-fixture", "untrusted-fixture"] {
        assert!(!root
            .path()
            .join("node_modules")
            .join(name)
            .join("ran.txt")
            .exists());
    }

    let summary = run(root.path(), &args(&["--run-scripts", "approved-fixture"]));
    assert_eq!(summary.ran, ["approved-fixture"]);
    assert_eq!(summary.skipped, ["untrusted-fixture"]);
    assert_eq!(
        fs::read_to_string(root.path().join("node_modules/approved-fixture/ran.txt")).unwrap(),
        "approved"
    );
    assert!(!root
        .path()
        .join("node_modules/untrusted-fixture/ran.txt")
        .exists());
}

#[test]
fn lifecycle_preserves_declared_name_in_script_environment() {
    let _env = crate::test_env_lock::env_lock();
    let root = TempDir::new().unwrap();
    write_package(root.path(), "approved-fixture", "esbuild");
    let manifest_path = root
        .path()
        .join("node_modules/approved-fixture/package.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["scripts"]["postinstall"] = json!("printf '%s' \"$npm_package_name\" > ran.txt");
    fs::write(manifest_path, manifest.to_string()).unwrap();

    run(root.path(), &args(&["--run-scripts", "approved-fixture"]));
    assert_eq!(
        fs::read_to_string(root.path().join("node_modules/approved-fixture/ran.txt")).unwrap(),
        "esbuild"
    );
}

#[test]
fn scanner_checks_declared_names_but_overrides_use_installed_names() {
    for (installed, declared, blocked) in [
        ("legitimate-alias", "expres", true),
        ("expres", "express", false),
    ] {
        let root = TempDir::new().unwrap();
        write_package(root.path(), installed, declared);
        let packages = scanner::discover_packages(&root.path().join("node_modules"));
        let report = scanner::scan_packages(&packages, &[], false);
        assert_eq!(
            matches!(report.verdict, scanner::report::Verdict::Blocked),
            blocked
        );
        if blocked {
            assert_eq!(report.findings.len(), 1);
            assert_eq!(report.findings[0].package, "legitimate-alias@1.0.0");
            assert!(report.findings[0]
                .message
                .contains("--allow-risky legitimate-alias"));
            assert!(report.findings[0]
                .message
                .contains("npmjs.com/package/expres"));
            let overridden = scanner::scan_packages(&packages, &[installed.into()], false);
            assert!(matches!(
                overridden.verdict,
                scanner::report::Verdict::Overridden
            ));
            assert!(overridden.findings[0].overridden);
        }
    }
}

#[test]
fn lifecycle_preserves_project_and_blanket_approvals() {
    let _env = crate::test_env_lock::env_lock();
    let root = TempDir::new().unwrap();
    write_package(root.path(), "@approved/alias", "@source/original");
    write_package(root.path(), "untrusted-fixture", "@source/original");
    fs::write(
        root.path().join("package.json"),
        json!({"perry": {"allowScripts": ["@approved/alias"]}}).to_string(),
    )
    .unwrap();

    let summary = run(root.path(), &args(&[]));
    assert_eq!(summary.ran, ["@approved/alias"]);
    assert_eq!(summary.skipped, ["untrusted-fixture"]);
    assert_eq!(
        fs::read_to_string(root.path().join("node_modules/@approved/alias/ran.txt")).unwrap(),
        "approved"
    );
    assert!(!root
        .path()
        .join("node_modules/untrusted-fixture/ran.txt")
        .exists());

    let mut summary = run(root.path(), &args(&["--run-scripts-all"]));
    summary.ran.sort();
    assert_eq!(summary.ran, ["@approved/alias", "untrusted-fixture"]);
    assert!(summary.skipped.is_empty());
    assert_eq!(
        fs::read_to_string(root.path().join("node_modules/untrusted-fixture/ran.txt")).unwrap(),
        "approved"
    );
}
