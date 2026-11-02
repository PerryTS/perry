//! The `Function` constructor reach is decided by binding, not by name.
//!
//! A module that reads `E.Function` from its own enum, uses a bare `Function`
//! member inside that enum's initializer, reads `X.Function` from an enum
//! another module exports, and indexes a parameter named `map` next to a
//! `function map` can never construct a function from a string. The
//! interpreter is not linked, so whole-program facts carry no interpreted-code
//! guard (`# interp_guard none`), in whole mode and per unit.
//!
//! The control constructs a function from a string: the guard is on, so the
//! check can fail.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn clean(cmd: &mut Command) -> &mut Command {
    for (key, _) in std::env::vars() {
        if key.starts_with("PERRY_") || key == "NODE_OPTIONS" {
            cmd.env_remove(key);
        }
    }
    cmd
}

/// Write `files` (the first is the entry), compile (whole program when
/// `whole`, else per unit), run; return (stdout, the facts dump header line
/// `# interp_guard ...`).
fn build(dir: &Path, files: &[(&str, &str)], whole: bool) -> (String, String) {
    for (name, text) in files {
        std::fs::write(dir.join(name), text).unwrap();
    }
    let tag = if whole { "w" } else { "u" };
    let exe = dir.join(format!("main_{tag}"));
    let dump = dir.join(format!("facts_{tag}"));
    let mut cmd = Command::new(perry_bin());
    clean(&mut cmd)
        .current_dir(dir)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_FACTS_DUMP", &dump)
        .env("PERRY_FACTS_UNITS", if whole { "0" } else { "1" });
    let out = cmd
        .arg("compile")
        .arg(dir.join(files[0].0))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("compile");
    assert!(
        out.status.success(),
        "compile failed (whole={whole})\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut run = Command::new(&exe);
    let ran = clean(&mut run).output().expect("run");
    assert!(ran.status.success(), "run failed (whole={whole}): {:?}", ran.status);
    let facts = std::fs::read_to_string(&dump).expect("facts dump");
    let guard = facts
        .lines()
        .find(|l| l.starts_with("# interp_guard "))
        .unwrap_or_else(|| panic!("no interp_guard header (whole={whole})"))
        .to_string();
    (String::from_utf8_lossy(&ran.stdout).into_owned(), guard)
}

const KINDS: &str = "\
export enum Kind { Value = 1, Function = 2, Both = Value | Function }
";

const NAMES: &str = "\
import { Kind } from './kinds';
enum Local { Value = 1, Function = 4, Mixed = Value | Function }
function map(xs: number[], f: (x: number) => number): number[] {
  const out: number[] = [];
  for (const x of xs) out.push(f(x));
  return out;
}
function pick(map: Record<string, number>, key: string): number {
  return map[key];
}
const flags = Local.Function | Kind.Function;
console.log(flags, Local.Mixed, Kind.Both, pick({ a: 3 }, 'a'), map([1, 2], (x) => x * 2).join(','));
";

#[test]
fn enum_members_and_shadowing_names_do_not_link_the_interpreter() {
    let dir = tempfile::tempdir().unwrap();
    for whole in [true, false] {
        let (stdout, guard) = build(
            dir.path(),
            &[("main.ts", NAMES), ("kinds.ts", KINDS)],
            whole,
        );
        assert_eq!(stdout.trim(), "6 5 3 3 2,4", "whole={whole}");
        assert_eq!(guard, "# interp_guard none", "whole={whole}");
    }
}

const CONTROL: &str = "\
enum Local { Value = 1, Function = 4 }
const body = 'return a + ' + String(Local.Function);
const f = new Function('a', body) as (a: number) => number;
console.log(f(1));
";

#[test]
fn a_function_built_from_a_string_keeps_the_guard() {
    let dir = tempfile::tempdir().unwrap();
    for whole in [true, false] {
        let (stdout, guard) = build(dir.path(), &[("main.ts", CONTROL)], whole);
        assert_eq!(stdout.trim(), "5", "whole={whole}");
        assert_ne!(guard, "# interp_guard none", "whole={whole}");
    }
}
