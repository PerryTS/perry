//! Owner 11-24 C1 (D2): no separate activation pass where none can matter.
//! Dynamic reach drops only modules the static edges do not reach; when the
//! static edges alone reach every module, it runs no activation pass and the
//! facts take their activation from their own final solve.
//!
//! * every dynamic target also reached through a static edge: no
//!   `activation pass` line, one `activation (final solve)` line, nothing
//!   dropped, the binary runs;
//! * a dead dynamic-only target is dropped and a live one kept (the pass
//!   still decides there), and the binary runs.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Whole compile with the facts' profile and the dynamic reach report;
/// returns (stderr, report, the binary's stdout).
fn compile_and_run(dir: &Path, files: &[(&str, &str)]) -> (String, String, String) {
    for (name, text) in files {
        std::fs::write(dir.join(name), text).unwrap();
    }
    let report = dir.join("reach.txt");
    let bin = dir.join("main_bin");
    let mut cmd = Command::new(perry_bin());
    for (key, _) in std::env::vars() {
        if key.starts_with("PERRY_FACTS") || key.starts_with("PERRY_DYNAMIC") {
            cmd.env_remove(key);
        }
    }
    let out = cmd
        .current_dir(dir)
        .env("PERRY_FACTS_PROFILE", "1")
        .env("PERRY_DYNAMIC_REACH_REPORT", &report)
        .arg("compile")
        .arg(dir.join("main.ts"))
        .arg("-o")
        .arg(&bin)
        .arg("--no-cache")
        .output()
        .expect("compile");
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "compile failed\n{err}");
    let run = Command::new(&bin)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(run.status.success(), "binary failed\n{err}");
    (
        err,
        std::fs::read_to_string(&report).unwrap_or_default(),
        String::from_utf8_lossy(&run.stdout).into_owned(),
    )
}

fn count(err: &str, prefix: &str) -> usize {
    err.lines().filter(|l| l.starts_with(prefix)).count()
}

const PASS: &str = "perry: wpfacts activation pass:";
const FINAL: &str = "perry: wpfacts activation (final solve):";

#[test]
fn statically_reached_dynamic_targets_run_no_activation_pass() {
    let dir = tempfile::tempdir().unwrap();
    let (err, report, stdout) = compile_and_run(
        dir.path(),
        &[
            (
                "main.ts",
                r#"
import { shared } from "./shared.ts";
import { other } from "./other.ts";
import { load, never } from "./loader.ts";
async function main() {
  const m: any = await load();
  console.log(shared, other, m.other, typeof never);
}
main();
"#,
            ),
            (
                "loader.ts",
                r#"
export function load(): Promise<unknown> { return import("./other.ts"); }
export function never(): Promise<unknown> { return import("./shared.ts"); }
"#,
            ),
            ("shared.ts", "export const shared = 'shared';\n"),
            ("other.ts", "export const other = 'other';\n"),
        ],
    );
    assert!(
        err.contains(
            "every module reached through static edges; nothing to drop, no activation pass"
        ),
        "{err}"
    );
    assert_eq!(count(&err, PASS), 0, "no separate activation pass\n{err}");
    assert_eq!(
        count(&err, FINAL),
        1,
        "D2: activation from the final solve\n{err}"
    );
    assert!(!report.contains("dropped "), "{report}");
    assert_eq!(stdout, "shared other other function\n");
}

#[test]
fn dead_dynamic_only_target_is_dropped_and_live_one_kept() {
    let dir = tempfile::tempdir().unwrap();
    let (err, report, stdout) = compile_and_run(
        dir.path(),
        &[
            (
                "main.ts",
                r#"
function never(): Promise<unknown> { return import("./dead.ts"); }
async function main() {
  const lazy = process.argv.length < 99 ? await import("./lazy.ts") : null;
  console.log(lazy === null ? "no" : lazy.value, typeof never);
}
main();
"#,
            ),
            ("lazy.ts", "export const value = 'lazy';\n"),
            ("dead.ts", "export const value = 'dead';\n"),
        ],
    );
    assert!(
        err.contains("dynamic reach: 2 dynamic edges, 1 held only by bodies that cannot run; 1 modules dropped"),
        "{err}"
    );
    let dropped: Vec<&str> = report
        .lines()
        .filter(|l| l.starts_with("dropped "))
        .collect();
    assert_eq!(dropped.len(), 1, "{report}");
    assert!(dropped[0].ends_with("dead.ts"), "{report}");
    assert!(
        report
            .lines()
            .any(|l| l.starts_with("edge live") && l.ends_with("-> ./lazy.ts")),
        "{report}"
    );
    assert_eq!(stdout, "lazy function\n");
}
