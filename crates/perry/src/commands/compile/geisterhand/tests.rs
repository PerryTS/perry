use super::*;
use serde_json::json;

fn messages(dir: &Path, extension: &str, fresh: bool) -> Vec<u8> {
    let mut output = Vec::new();
    for name in [
        "perry_runtime",
        "perry_stdlib",
        "perry_ui_macos",
        "perry_ui_geisterhand",
    ] {
        let archive = dir.join(format!("{name}.{extension}"));
        std::fs::write(&archive, b"archive").unwrap();
        let record = json!({
            "reason": "compiler-artifact",
            "target": {"name": name, "crate_types": ["rlib", "staticlib"]},
            "filenames": [dir.join(format!("{name}.rlib")), archive],
            "fresh": fresh,
        });
        serde_json::to_writer(&mut output, &record).unwrap();
        output.push(b'\n');
    }
    output
}

#[test]
fn uses_reported_paths_for_both_new_and_fresh_archives() {
    let dir = tempfile::tempdir().unwrap();
    // Cargo may choose a triple directory even without an explicit --target.
    let artifacts = dir
        .path()
        .join("custom-target/aarch64-apple-darwin/release");
    std::fs::create_dir_all(&artifacts).unwrap();
    for extension in ["a", "lib"] {
        for fresh in [false, true] {
            let mut output = messages(&artifacts, extension, fresh);
            // The underlying rlib has the same name as the static wrapper.
            output.extend_from_slice(br#"{"reason":"compiler-artifact","target":{"name":"perry_runtime","crate_types":["rlib"]},"filenames":["unrelated/perry_runtime.rlib"]}"#);
            let libs = artifacts_from_messages(&output, "perry-ui-macos").unwrap();
            assert_eq!(
                libs.runtime,
                artifacts.join(format!("perry_runtime.{extension}"))
            );
            assert_eq!(
                libs.stdlib,
                artifacts.join(format!("perry_stdlib.{extension}"))
            );
            assert_eq!(
                libs.ui,
                artifacts.join(format!("perry_ui_macos.{extension}"))
            );
            assert_eq!(
                libs.server,
                artifacts.join(format!("perry_ui_geisterhand.{extension}"))
            );
        }
    }
}

#[test]
fn incomplete_build_does_not_fall_back_to_existing_archives() {
    let dir = tempfile::tempdir().unwrap();
    let output = messages(dir.path(), "a", true);
    let first_line = output.split(|b| *b == b'\n').next().unwrap();
    let error = artifacts_from_messages(first_line, "perry-ui-macos").unwrap_err();
    assert!(error.to_string().contains("perry_stdlib"), "{error}");
    std::fs::remove_file(dir.path().join("perry_stdlib.a")).unwrap();
    let error = artifacts_from_messages(&output, "perry-ui-macos").unwrap_err();
    assert!(
        error.to_string().contains("missing Geisterhand library"),
        "{error}"
    );
}

#[test]
fn conflicting_artifacts_are_rejected() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let mut output = messages(first.path(), "a", false);
    output.extend(messages(second.path(), "a", true));
    let error = artifacts_from_messages(&output, "perry-ui-macos").unwrap_err();
    assert!(
        error.to_string().contains("multiple Geisterhand archives"),
        "{error}"
    );
}

#[cfg(unix)]
#[test]
fn failed_build_does_not_use_reported_or_cached_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    let output = messages(dir.path(), "a", true);
    let file = dir.path().join("artifacts.jsonl");
    std::fs::write(&file, output).unwrap();
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "cat \"$1\"; exit 1", "cargo"]).arg(file);
    let error = run_build(&mut cmd, "perry-ui-macos", 0).unwrap_err();
    assert!(
        error.to_string().contains("Failed to build Geisterhand"),
        "{error}"
    );
}

// Exercise the real Cargo command against a tiny workspace with the same
// runtime/stdlib/static-wrapper graph. Two independently built feature variants
// have separate Rust TLS state; a single graph must make both archives observe
// the same value. This detects the duplicate-runtime mechanism without relying
// on ASLR, HashMap layout, a full stdlib build, or a running desktop session.
#[cfg(unix)]
#[test]
fn warm_archives_are_rebuilt_as_one_runtime_graph() {
    fn write_crate(root: &Path, package: &str, name: &str, kind: &str, extra: &str, source: &str) {
        let dir = root.join(package);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[package]\nname = {package:?}\nversion = \"0.0.0\"\nedition = \"2021\"\n\
             [lib]\nname = {name:?}\ncrate-type = [{kind:?}]\n{extra}\n"
            ),
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), source).unwrap();
    }
    fn success(cmd: &mut Command) -> std::process::Output {
        let output = cmd.output().unwrap();
        assert!(
            output.status.success(),
            "{cmd:?}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("Cargo.toml"), r#"
[workspace]
resolver = "2"
members = ["perry-runtime", "perry-runtime-static", "perry-stdlib-static", "perry-ui-macos", "perry-ui-geisterhand"]
"#).unwrap();
    write_crate(
        root,
        "perry-runtime",
        "perry_runtime",
        "rlib",
        r#"
[features]
geisterhand = []
stdlib = []
"#,
        r#"
thread_local! { static VALUE: std::cell::Cell<i32> = const { std::cell::Cell::new(0) }; }
#[inline(never)] pub fn set(value: i32) { VALUE.with(|v| v.set(value)); }
#[inline(never)] pub fn get() -> i32 { VALUE.with(|v| v.get()) }
"#,
    );
    write_crate(
        root,
        "perry-runtime-static",
        "perry_runtime",
        "staticlib",
        r#"
[dependencies]
perry-runtime = { path = "../perry-runtime" }
"#,
        r#"
#[no_mangle] pub extern "C" fn runtime_set(value: i32) { perry_runtime::set(value); }
"#,
    );
    write_crate(
        root,
        "perry-stdlib-static",
        "perry_stdlib",
        "staticlib",
        r#"
[dependencies]
perry-runtime = { path = "../perry-runtime", features = ["stdlib"] }
"#,
        r#"
#[no_mangle] pub extern "C" fn stdlib_get() -> i32 { perry_runtime::get() }
"#,
    );
    write_crate(
        root,
        "perry-ui-macos",
        "perry_ui_macos",
        "staticlib",
        "[features]\ngeisterhand = []",
        "",
    );
    write_crate(
        root,
        "perry-ui-geisterhand",
        "perry_ui_geisterhand",
        "staticlib",
        "",
        "",
    );
    let target_dir = root.join("target/geisterhand");
    let host = super::super::host_target_triple().expect("rustc host triple");
    let cargo = |packages: &[&str]| {
        let mut cmd = Command::new("cargo");
        cmd.current_dir(root)
            .env("CARGO_TARGET_DIR", &target_dir)
            .env("CARGO_BUILD_TARGET", host)
            .args(["build", "--release"])
            .args(packages);
        cmd
    };
    success(&mut cargo(&[
        "-p",
        "perry-runtime-static",
        "--features",
        "perry-runtime/geisterhand",
    ]));
    success(&mut cargo(&[
        "-p",
        "perry-stdlib-static",
        "-p",
        "perry-ui-macos",
        "-p",
        "perry-ui-geisterhand",
    ]));
    let release = target_dir.join(host).join("release");
    let source = root.join("probe.c");
    std::fs::write(
        &source,
        r#"
#include <stdio.h>
void runtime_set(int);
int stdlib_get(void);
int main(void) { runtime_set(41); printf("%d\n", stdlib_get()); }
"#,
    )
    .unwrap();
    let probe = |runtime: &Path, stdlib: &Path| {
        let exe = root.join("probe");
        let mut cc = Command::new("cc");
        cc.arg(&source).arg(runtime).arg(stdlib).arg("-o").arg(&exe);
        if cfg!(target_os = "linux") {
            cc.args(["-ldl", "-lpthread", "-lm"]);
        }
        success(&mut cc);
        String::from_utf8(success(&mut Command::new(&exe)).stdout).unwrap()
    };
    assert_eq!(
        probe(
            &release.join("libperry_runtime.a"),
            &release.join("libperry_stdlib.a")
        ),
        "0\n",
        "fixture must contain distinct runtime instances before the fix"
    );
    // Even without a Perry --target, Cargo can put outputs under a triple.
    let mut build = build_command(root, "perry-ui-macos", None);
    build.env("CARGO_BUILD_TARGET", host);
    let libs = run_build(&mut build, "perry-ui-macos", 0).unwrap();
    assert_eq!(probe(&libs.runtime, &libs.stdlib), "41\n");
    // Cargo reports fresh artifacts too; a second preparation must keep working.
    let fresh = run_build(&mut build, "perry-ui-macos", 0).unwrap();
    assert_eq!(fresh.runtime, libs.runtime);
    assert_eq!(probe(&fresh.runtime, &fresh.stdlib), "41\n");
}
