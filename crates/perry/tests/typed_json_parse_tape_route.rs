//! `JSON.parse<T[]>` routes through the same tape gate as the generic entry.
//!
//! The schema-directed typed parse (#179 Step 1b) was written against the
//! pre-tape `DirectParser`, and Step 2 then made the tape-based lazy parse
//! the GENERIC default for exactly the payloads the specialization targets.
//! Nobody went back: the "fast path" parsed 4x slower than the generic
//! parser it claims to specialize (589ms vs 144ms on the typed-roundtrip
//! benchmark's blob), and its eagerly materialized output re-stringified 8x
//! slower than the tape's lazy values (580ms vs 72ms).
//!
//! Both entries now share one `tape_route_eligible` predicate, and the typed
//! entry delegates to the generic one whenever the tape qualifies — licensed
//! by its own documented contract ("no user-visible difference from
//! `JSON.parse(blob) as T[]`"). The shape hint keeps the window the tape
//! declines: sub-1KB payloads, >16MB payloads, non-array roots.
//!
//! These tests pin OUTPUT EQUALITY across every route: typed vs untyped
//! parse, under the tape's three env modes, both blob-size windows, and a
//! moving collector. If a future change makes any route disagree, the
//! roundtrip strings diverge.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = r#"
interface Item {
  id: number;
  name: string;
  value: number;
  tags: string[];
  nested: { x: number; y: number };
}
// Two blobs: one above the 1KB tape floor (tape route in auto mode), one
// below it (shape-hint route in every mode).
function build(n: number): string {
  const items: Item[] = [];
  for (let i = 0; i < n; i++) {
    items.push({
      id: i,
      name: "item_" + i,
      value: i * 3.14159,
      tags: ["tag_" + (i % 10), "tag_" + (i % 5)],
      nested: { x: i, y: i * 2 }
    });
  }
  return JSON.stringify(items);
}
const big = build(200);
const tiny = build(3);
for (const blob of [big, tiny]) {
  const typed = JSON.parse<Item[]>(blob);
  const untyped = JSON.parse(blob) as Item[];
  const a = JSON.stringify(typed);
  const b = JSON.stringify(untyped);
  console.log("len:" + typed.length + " eq_untyped:" + (a === b) + " eq_blob:" + (a === blob));
  console.log("probe:" + typed[1].name + ":" + typed[1].nested.y + ":" + typed[1].tags[1]);
}
"#;

const EXPECTED: &str = "len:200 eq_untyped:true eq_blob:true\nprobe:item_1:2:tag_1\nlen:3 eq_untyped:true eq_blob:true\nprobe:item_1:2:tag_1\n";

fn compile(dir: &Path) -> PathBuf {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write entry");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    output
}

fn run(bin: &Path, dir: &Path, tape_mode: Option<&str>, moving_gc: bool) -> Output {
    let mut command = Command::new(bin);
    command.current_dir(dir);
    if let Some(mode) = tape_mode {
        command.env("PERRY_JSON_TAPE", mode);
    }
    if moving_gc {
        command
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_VERIFY_EVACUATION", "1");
    }
    command.output().expect("run compiled binary")
}

/// Every route agrees: tape auto (typed delegates for the big blob, shape
/// path for the tiny one), forced tape, forced direct — with and without a
/// moving collector. `eq_blob` doubles as node-parity: the blob was produced
/// by stringify, so a byte-identical re-stringify pins field order, number
/// formatting, and escaping through every route.
#[test]
fn typed_and_untyped_parse_agree_across_every_tape_mode() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin = compile(dir.path());
    for tape_mode in [None, Some("0"), Some("1")] {
        for moving_gc in [false, true] {
            let output = run(&bin, dir.path(), tape_mode, moving_gc);
            assert!(
                output.status.success(),
                "binary failed with tape={tape_mode:?} moving_gc={moving_gc}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                EXPECTED,
                "route disagreement with tape={tape_mode:?} moving_gc={moving_gc}"
            );
        }
    }
}
