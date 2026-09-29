//! Design step 4: the static shape seed set.
//!
//! Generated guards compare a literal shape against its static ShapeId as an
//! immediate. The seed unit mints those ids in every agent before any user
//! code runs, so an earlier module that builds the same key list by a
//! non-static path (JSON.parse, dynamic key adds, Object.fromEntries) cannot
//! take the facts under a counter id first. The census
//! (`PERRY_STATIC_SHAPE_CENSUS=1`) names every static request and whether the
//! mint returned the requested id: a `hit` on the literal's own mint means its
//! fresh objects carry exactly the immediate its guards compare against.
//!
//! The seed set must be the same cold and warm (the object cache replays it
//! from a sidecar), and the test must go red when the seed is sabotaged.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Built first (main imports it): three non-static births of `{lt_u, lt_v}`
/// with keys the compiler cannot see.
const EARLY: &str = r#"
const ku = ["lt", "u"].join("_");
const kv = ["lt", "v"].join("_");
export const viaJson: any = JSON.parse("{\"" + ku + "\":1,\"" + kv + "\":2}");
const d: any = {};
d[ku] = 3;
d[kv] = 4;
export const viaAdds: any = d;
export const viaEntries: any = Object.fromEntries([[ku, 5], [kv, 6]]);
"#;

/// A literal of the same key list read through a parameter receiver, so its
/// field reads are guarded against the literal's static id.
const MAIN: &str = r#"
import { viaJson, viaAdds, viaEntries } from "./early";
type P = { lt_u: number; lt_v: number };
function sum(ps: P[]): number {
  let s = 0;
  for (let i = 0; i < ps.length; i++) { const p = ps[i]; s += p.lt_u * 3 + p.lt_v; }
  return s;
}
const ps: P[] = [];
for (let i = 0; i < 100; i++) ps.push({ lt_u: i, lt_v: i + 1 });
console.log(sum(ps), viaJson.lt_u + viaAdds.lt_v + viaEntries.lt_u);
"#;

const EXPECTED: &str = "19900 10";

fn compile(dir: &Path, out: &str) -> (PathBuf, String) {
    let output = dir.join(out);
    let run = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(dir.join("main.ts"))
        .arg("-o")
        .arg(&output)
        .arg("-v")
        .env_remove("PERRY_NO_CACHE")
        .env("PERRY_CACHE_DIR", dir.join("cache"))
        .output()
        .expect("run perry compile");
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(run.status.success(), "perry compile failed:\n{log}");
    (output, log)
}

/// Run `bin` with the census on: its stdout and the census lines.
fn census(bin: &Path) -> (String, Vec<String>) {
    let run = Command::new(bin)
        .env("PERRY_STATIC_SHAPE_CENSUS", "1")
        .output()
        .expect("run the compiled program");
    assert!(run.status.success(), "the program failed: {run:?}");
    let lines = String::from_utf8_lossy(&run.stderr)
        .lines()
        .filter(|l| l.starts_with("perry-static-shape: "))
        .map(str::to_string)
        .collect();
    (String::from_utf8_lossy(&run.stdout).trim().to_string(), lines)
}

/// The static ids the seed unit minted (each must be a hit).
fn seeded(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|l| l.starts_with("perry-static-shape: seed "))
        .map(|l| {
            assert!(l.ends_with(" hit"), "a seed did not adopt its id: {l}");
            l.split_whitespace().nth(2).expect("requested=").to_string()
        })
        .collect()
}

/// The literal mints (`class` requests) of the seeded ids.
fn literal_mints<'a>(lines: &'a [String], ids: &[String]) -> Vec<&'a String> {
    lines
        .iter()
        .filter(|l| l.starts_with("perry-static-shape: class "))
        .filter(|l| ids.iter().any(|id| l.split_whitespace().nth(2) == Some(id)))
        .collect()
}

#[test]
fn the_seed_makes_a_literals_births_carry_its_guard_immediate_cold_and_warm() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("early.ts"), EARLY).unwrap();
    std::fs::write(dir.path().join("main.ts"), MAIN).unwrap();

    // Cold: codegen reports the seeds and stores them beside the objects.
    let (cold, log) = compile(dir.path(), "cold_bin");
    assert!(log.contains("Stored cached object"), "cold build: {log}");
    let (out, lines) = census(&cold);
    assert_eq!(out, EXPECTED);
    let ids = seeded(&lines);
    assert!(
        !ids.is_empty(),
        "no literal seed: the guard immediates were not captured\n{lines:#?}"
    );
    let mints = literal_mints(&lines, &ids);
    assert!(!mints.is_empty(), "the literal's own mint never ran\n{lines:#?}");
    for l in &mints {
        assert!(l.ends_with(" hit"), "cold: the literal's births miss its immediate: {l}");
    }

    // Warm: every module is a cache hit, and the replayed sidecars give the
    // same seed set.
    let (warm, log) = compile(dir.path(), "warm_bin");
    assert!(log.contains("Reused cached object"), "warm build: {log}");
    let (out, warm_lines) = census(&warm);
    assert_eq!(out, EXPECTED);
    assert_eq!(seeded(&warm_lines), ids, "the warm seed set differs from the cold one");
    for l in literal_mints(&warm_lines, &ids) {
        assert!(l.ends_with(" hit"), "warm: the literal's births miss its immediate: {l}");
    }

    // Sabotage: empty seed sidecars make the warm link seed nothing. The
    // early module then takes the literal's facts under a counter id, and
    // the census must show the literal's mint MISS — the witness above can
    // fail.
    let objects = dir.path().join("cache");
    let mut emptied = 0;
    for entry in walk(&objects) {
        if entry.extension().and_then(|e| e.to_str()) == Some("seeds") {
            std::fs::write(&entry, "").unwrap();
            emptied += 1;
        }
    }
    assert!(emptied > 0, "no seed sidecar was written under {objects:?}");
    let (sabotaged, log) = compile(dir.path(), "sabotaged_bin");
    assert!(log.contains("Reused cached object"), "sabotaged build: {log}");
    let (out, sab_lines) = census(&sabotaged);
    assert_eq!(out, EXPECTED, "a miss must only be slow");
    assert!(seeded(&sab_lines).is_empty(), "the sabotaged build still seeded");
    let sab_mints = literal_mints(&sab_lines, &ids);
    assert!(
        !sab_mints.is_empty() && sab_mints.iter().all(|l| l.ends_with(" miss")),
        "without the seed the literal's mint must miss (else this test cannot fail)\n{sab_lines:#?}"
    );
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}
