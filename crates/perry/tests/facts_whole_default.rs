//! DECISIONS 251: the facts codegen reads are the whole-program solve's.
//!
//! With no `PERRY_FACTS_UNITS` in the environment a compile solves the
//! facts over the whole program, where a body contributes only once the
//! program can run it (DECISIONS 225). The fixture tells the modes apart by
//! construction: `q.ts` exports `hook`, which nothing calls, and `hook` hands
//! the `Diagnostics` literal of `a.ts` to a function of unknown code. The
//! whole solve never walks `hook`, so the reads of `Diagnostics.Alpha.code`
//! and `Diagnostics.Beta.message` in `c.ts` are proven. The per-unit solve
//! walks every body of every unit, the escape breaks the literal's shapes,
//! and the same reads are ⊤ (`receiver from unmodeled code`). The dump is
//! the text of the `ProgramFacts` handed to codegen (`program_facts.rs`), so
//! the assertion is on what codegen reads. Red while per unit is the default.

use std::process::Command;

const A_TS: &str = r#"
function diag(code: number, message: string) {
  return { code, message };
}
export const Diagnostics = {
  Alpha: diag(1001, "Alpha {0}"),
  Beta: diag(1002, "Beta {1}"),
};
"#;

const C_TS: &str = r#"
import { Diagnostics } from "./a";
export function bind(): number {
  return Diagnostics.Alpha.code + Diagnostics.Beta.message.length;
}
"#;

/// Exported and never called: the whole solve never walks it.
const Q_TS: &str = r#"
import { Diagnostics } from "./a";
export function hook(): void {
  (globalThis as any).perryHook({ diagnostics: Diagnostics });
}
"#;

const MAIN_TS: &str = r#"
import { bind } from "./c";
import { hook } from "./q";
export const keep = hook;
console.log(bind());
"#;

fn clean(cmd: &mut Command) -> &mut Command {
    for (key, _) in std::env::vars() {
        if key.starts_with("PERRY_") || key == "NODE_OPTIONS" {
            cmd.env_remove(key);
        }
    }
    cmd
}

/// The verdicts of the dump's `S` records for `key`
/// (`S <module> <offset> <key> ... verdict=<verdict>`).
fn verdicts(dump: &str, key: &str) -> Vec<String> {
    dump.lines()
        .filter(|l| l.starts_with("S ") && l.split(' ').nth(3) == Some(key))
        .map(|l| l.rsplit("verdict=").next().unwrap_or("").trim().to_string())
        .collect()
}

#[test]
fn codegen_reads_the_whole_program_solve_by_default() {
    let dir = tempfile::tempdir().unwrap();
    for (name, src) in [
        ("a.ts", A_TS),
        ("c.ts", C_TS),
        ("q.ts", Q_TS),
        ("main.ts", MAIN_TS),
    ] {
        std::fs::write(dir.path().join(name), src).unwrap();
    }
    let exe = dir.path().join("main_bin");
    let dump = dir.path().join("facts.dump");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_perry"));
    clean(&mut cmd)
        .current_dir(dir.path())
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_FACTS_DUMP", &dump);
    // The default, not an opt-in: no facts-mode variable reaches the compile.
    assert!(cmd.get_envs().all(|(k, _)| k != "PERRY_FACTS_UNITS"));
    let out = cmd
        .arg("compile")
        .arg("main.ts")
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("compile");
    assert!(
        out.status.success(),
        "compile failed\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut run = Command::new(&exe);
    let ran = clean(&mut run).output().expect("run");
    assert!(ran.status.success(), "run failed: {:?}", ran.status);
    assert_eq!(
        String::from_utf8_lossy(&ran.stdout).trim(),
        "1009",
        "node prints 1009"
    );

    let facts = std::fs::read_to_string(&dump).expect("facts dump");
    for key in [".code", ".message"] {
        let v = verdicts(&facts, key);
        assert!(!v.is_empty(), "no {key} read site in the dump\n{facts}");
        assert!(
            v.iter().all(|v| !v.starts_with("no")),
            "the {key} read is not proven: codegen did not receive the whole solve\n{v:?}"
        );
    }
}
