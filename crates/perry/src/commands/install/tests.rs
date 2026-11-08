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

fn isolated_alias(root: &Path, store: &str, alias: &str) -> std::path::PathBuf {
    let installed = format!("{store}/esbuild@1.0.0/node_modules/esbuild");
    write_package(root, &installed, "esbuild");
    let physical = root.join("node_modules").join(installed);
    let manifest_path = physical.join("package.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["scripts"]["postinstall"] = json!("printf approved >> ran.txt");
    fs::write(manifest_path, manifest.to_string()).unwrap();
    std::os::unix::fs::symlink(&physical, root.join("node_modules").join(alias)).unwrap();
    physical
}

#[test]
fn lifecycle_store_names_cannot_approve_visible_aliases() {
    let _env = crate::test_env_lock::env_lock();
    for store in [".bun", ".pnpm"] {
        let root = TempDir::new().unwrap();
        let physical = isolated_alias(root.path(), store, "untrusted-fixture");

        let summary = run(root.path(), &args(&["--run-scripts", "esbuild"]));
        assert!(summary.ran.is_empty());
        assert_eq!(summary.skipped, ["untrusted-fixture"]);
        assert!(!physical.join("ran.txt").exists());

        let summary = run(root.path(), &args(&["--run-scripts", "untrusted-fixture"]));
        assert_eq!(summary.ran, ["untrusted-fixture"]);
        assert!(summary.skipped.is_empty());
        assert_eq!(
            fs::read_to_string(physical.join("ran.txt")).unwrap(),
            "approved"
        );
    }
}

#[test]
fn lifecycle_runs_a_shared_physical_package_once() {
    let _env = crate::test_env_lock::env_lock();
    for store in [".bun", ".pnpm"] {
        for flags in [
            vec![
                "--run-scripts",
                "first-alias",
                "--run-scripts",
                "second-alias",
            ],
            vec!["--run-scripts-all"],
        ] {
            let root = TempDir::new().unwrap();
            let physical = isolated_alias(root.path(), store, "first-alias");
            std::os::unix::fs::symlink(&physical, root.path().join("node_modules/second-alias"))
                .unwrap();
            let summary = run(root.path(), &args(&flags));
            assert_eq!(summary.ran.len(), 1);
            assert!(["first-alias", "second-alias"].contains(&summary.ran[0].as_str()));
            assert!(summary.skipped.is_empty());
            assert_eq!(
                fs::read_to_string(physical.join("ran.txt")).unwrap(),
                "approved"
            );
        }
    }
}

#[test]
fn lifecycle_follows_scoped_dependencies_without_looping_over_link_cycles() {
    let _env = crate::test_env_lock::env_lock();
    for store in [".bun", ".pnpm"] {
        let root = TempDir::new().unwrap();
        let outer = isolated_alias(root.path(), store, "outer-alias");
        let nested_path = format!("{store}/nested-fixture@1.0.0/node_modules/nested-fixture");
        write_package(root.path(), &nested_path, "nested-fixture");
        let nested = root.path().join("node_modules").join(nested_path);
        fs::create_dir_all(outer.join("node_modules/@approved")).unwrap();
        std::os::unix::fs::symlink(&nested, outer.join("node_modules/@approved/native")).unwrap();
        fs::create_dir_all(nested.join("node_modules")).unwrap();
        std::os::unix::fs::symlink(&outer, nested.join("node_modules/back-link")).unwrap();

        let summary = run(root.path(), &args(&["--run-scripts", "@approved/native"]));
        assert_eq!(summary.ran, ["@approved/native"]);
        assert_eq!(summary.skipped.len(), 2);
        assert!(!outer.join("ran.txt").exists());
        assert_eq!(
            fs::read_to_string(nested.join("ran.txt")).unwrap(),
            "approved"
        );

        let summary = run(root.path(), &args(&["--run-scripts-all"]));
        assert_eq!(summary.ran.len(), 2);
        assert!(summary.skipped.is_empty());
        assert_eq!(
            fs::read_to_string(outer.join("ran.txt")).unwrap(),
            "approved"
        );
    }
}

#[test]
fn lifecycle_follows_dependencies_beside_isolated_packages() {
    let _env = crate::test_env_lock::env_lock();
    for store in [".bun", ".pnpm"] {
        for physical_name in ["outer-fixture", "@source/original"] {
            let root = TempDir::new().unwrap();
            let container = format!("{store}/outer@1.0.0/node_modules");
            let outer_path = format!("{container}/{physical_name}");
            write_package(root.path(), &outer_path, "outer-fixture");
            let modules = root.path().join("node_modules");
            let outer = modules.join(outer_path);
            std::os::unix::fs::symlink(&outer, modules.join("outer-alias")).unwrap();
            let nested_path = format!("{store}/nested@1.0.0/node_modules/nested-fixture");
            write_package(root.path(), &nested_path, "nested-fixture");
            let nested = modules.join(nested_path);
            let scope = modules.join(container).join("@approved");
            fs::create_dir_all(&scope).unwrap();
            std::os::unix::fs::symlink(&nested, scope.join("nested")).unwrap();

            let summary = run(root.path(), &args(&["--run-scripts", "@approved/nested"]));
            assert_eq!(summary.ran, ["@approved/nested"]);
            assert_eq!(summary.skipped, ["outer-alias"]);
            assert!(!outer.join("ran.txt").exists());
            assert_eq!(
                fs::read_to_string(nested.join("ran.txt")).unwrap(),
                "approved"
            );

            fs::remove_file(nested.join("ran.txt")).unwrap();
            let summary = run(root.path(), &args(&["--run-scripts", physical_name]));
            assert!(summary.ran.is_empty());
            assert!(!outer.join("ran.txt").exists());
            assert!(!nested.join("ran.txt").exists());
        }
    }
}

#[test]
fn lifecycle_resolution_error_identifies_package_and_prior_progress() {
    let _env = crate::test_env_lock::env_lock();
    let root = TempDir::new().unwrap();
    for name in ["early-unapproved", "first-approved", "later-approved"] {
        write_package(root.path(), name, name);
    }
    let mut packages = scanner::discover_packages(&root.path().join("node_modules"));
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    fs::remove_dir_all(root.path().join("node_modules/later-approved")).unwrap();
    let error = lifecycle::run_all(
        &packages,
        root.path(),
        &args(&[
            "--run-scripts",
            "first-approved",
            "--run-scripts",
            "later-approved",
        ]),
    )
    .err()
    .expect("missing approved package must fail");
    let message = error.to_string();
    assert!(message.contains("later-approved@1.0.0"), "{message}");
    assert!(message.contains("completed: [first-approved]"), "{message}");
    assert!(message.contains("skipped: [early-unapproved]"), "{message}");
    assert_eq!(
        fs::read_to_string(root.path().join("node_modules/first-approved/ran.txt")).unwrap(),
        "approved"
    );
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
            report.verdict,
            if blocked {
                scanner::report::Verdict::Blocked
            } else {
                scanner::report::Verdict::Clean
            }
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
