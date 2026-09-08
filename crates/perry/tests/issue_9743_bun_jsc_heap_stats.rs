//! Each import form must resolve and return a usable report in a fresh executable.
use std::process::Command;

const CHECKS: &str = include_str!("../../../test-files/_helpers/bun_jsc_heap_stats_9743.ts");

fn compile_and_run(preamble: &str) -> String {
    compile_source(&format!("{preamble}\n{CHECKS}"), true)
}

fn compile_source(source: &str, bun: bool) -> String {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("app");
    std::fs::write(&entry, source).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_perry"));
    command.arg("compile");
    if bun {
        command.args(["--platform", "bun"]);
    }
    let compile = command
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .output()
        .unwrap();
    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    assert!(compile.status.success(), "{diagnostics}");
    assert!(
        !diagnostics.contains("Could not resolve import 'bun:jsc'"),
        "{diagnostics}"
    );
    let run = Command::new(binary).output().unwrap();
    assert!(
        run.status.success(),
        "status={}\nstdout={}\nstderr={}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8(run.stdout).unwrap()
}

fn expected() -> &'static str {
    "heapStats shapes ok\nretained 256 0 255\nheapStats growth ok\n"
}

#[test]
fn static_and_namespace_imports_share_the_dynamic_and_require_function() {
    let stdout = compile_and_run(
        r#"
        import { heapStats } from 'bun:jsc';
        import * as namespace from 'bun:jsc';
        const dynamic = await import('bun:jsc');
        const required = require('bun:jsc');
        if (heapStats !== namespace.heapStats || heapStats !== dynamic.heapStats || heapStats !== required.heapStats)
            throw new Error('heapStats function identity differs by import form');
    "#,
    );
    assert_eq!(stdout, expected());
}

#[test]
fn dynamic_import_exposes_heap_stats() {
    assert_eq!(
        compile_and_run("const { heapStats } = await import('bun:jsc');"),
        expected()
    );
}

#[test]
fn require_exposes_heap_stats() {
    assert_eq!(
        compile_and_run("const { heapStats } = require('bun:jsc');"),
        expected()
    );
}

#[test]
fn runtime_only_import_installs_its_dispatch_without_the_bun_global() {
    let stdout = compile_source(
        r#"
        const { heapStats } = await import('bun:jsc');
        console.log(typeof heapStats, heapStats.length);
        const stats = heapStats(true);
        console.log(typeof stats.heapSize, typeof stats.objectTypeCounts);
    "#,
        false,
    );
    assert_eq!(stdout, "function 0\nnumber object\n");
}
