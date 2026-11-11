//! DECISIONS 225: a compile runs the whole program's activation pass ONCE.
//! Dynamic reach runs it (right after collection) and the whole-program
//! facts take its set of bodies that can run over (`perry_wpfacts::
//! Activation`); before, `compute` ran the same pass again.
//!
//! * a facts-only whole-program compile of a program with dynamic `import()`
//!   prints exactly one `activation pass` profile line;
//! * `PERRY_FACTS_ACTIVATION_CHECK=1` (the facts run their own pass over the
//!   final HIR) prints two, and they agree: same active count, same key
//!   digest (the hand-over invariant's detector);
//! * the dynamic reach line counts dynamic edges without
//!   `PERRY_DYNAMIC_REACH_REPORT` (it used to print 0 unless the report was
//!   on), and the module only a dead site loads is dropped.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn fixture(dir: &Path) -> PathBuf {
    std::fs::write(
        dir.join("main.ts"),
        r#"
function never(): Promise<unknown> { return import("./dead.ts"); }
async function main() {
  const lazy = process.argv.length > 99 ? await import("./lazy.ts") : null;
  console.log(lazy === null ? "no" : lazy.value, typeof never);
}
main();
"#,
    )
    .unwrap();
    std::fs::write(dir.join("lazy.ts"), "export const value = 'lazy';\n").unwrap();
    std::fs::write(dir.join("dead.ts"), "export const value = 'dead';\n").unwrap();
    dir.join("main.ts")
}

/// Facts-only whole-program compile; returns stderr.
fn facts_only(dir: &Path, check: bool) -> String {
    let src = fixture(dir);
    let mut cmd = Command::new(perry_bin());
    for (key, _) in std::env::vars() {
        if key.starts_with("PERRY_") || key == "NODE_OPTIONS" {
            cmd.env_remove(key);
        }
    }
    cmd.current_dir(dir)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_FACTS_UNITS", "0")
        .env("PERRY_FACTS_ONLY", "1")
        .env("PERRY_FACTS_PROFILE", "1");
    if check {
        cmd.env("PERRY_FACTS_ACTIVATION_CHECK", "1");
    }
    let out = cmd
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(dir.join("main_bin"))
        .output()
        .expect("compile");
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "compile failed\n{err}");
    err
}

/// The `activation pass` lines as (active count, key digest, past the
/// bound). The function counts differ by design: the facts' pass runs after
/// dynamic reach dropped `dead.ts`.
fn passes(err: &str) -> Vec<(String, String, bool)> {
    err.lines()
        .filter(|l| l.starts_with("perry: wpfacts activation pass:"))
        .map(|l| {
            let w: Vec<&str> = l.split_whitespace().collect();
            let keys = w.iter().position(|t| *t == "keys").unwrap();
            (
                w[4].to_owned(),
                w[keys + 1].trim_end_matches(',').to_owned(),
                l.contains("past the bound"),
            )
        })
        .collect()
}

#[test]
fn one_activation_pass_per_compile() {
    let dir = tempfile::tempdir().unwrap();
    let err = facts_only(dir.path(), false);
    assert_eq!(
        passes(&err).len(),
        1,
        "one activation pass per compile\n{err}"
    );
    assert!(
        err.contains("dynamic reach: 2 dynamic edges, 1 held only by bodies that cannot run; 1 modules dropped"),
        "dynamic reach counts its edges without the report\n{err}"
    );
}

#[test]
fn the_facts_own_pass_over_the_final_hir_agrees() {
    let dir = tempfile::tempdir().unwrap();
    let err = facts_only(dir.path(), true);
    let p = passes(&err);
    assert_eq!(p.len(), 2, "the check runs the facts' own pass too\n{err}");
    assert_eq!(
        p[0], p[1],
        "the handed-over pass equals the final-HIR pass\n{err}"
    );
    assert!(!p[0].2, "{err}");
}
