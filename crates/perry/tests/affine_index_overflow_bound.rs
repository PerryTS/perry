//! The affine index materialization cannot wrap i64 (#9294 follow-up).
//!
//! #9294 computed `a[<affine>]` indices in i64 on the claim that proven-i32
//! leaves cannot overflow it. True for one multiply (|i32 * i32| <= 2^62),
//! false beyond: `2^21 * 2^22 * (2^21 + k)` at `k = 0` is exactly 2^64, the
//! i64 computation wraps to 0, the wrapped index passes the unsigned bounds
//! check, and the fast path reads `a[0]` — the WRONG element, silently —
//! where JS computes the index in doubles, goes out of bounds, and yields
//! `undefined` (NaN after the add). Flagged by review on the follow-up PR.
//!
//! The fix is a static magnitude bound (`affine_index_magnitude_bound`):
//! interval arithmetic in i128 at match time with every leaf at its i32
//! extreme, admitting the tree only when its worst case fits i63 — so
//! admission costs nothing at run time and the matcher and the lowering
//! share one predicate. `i * size + k` (2^62 + 2^31) stays admitted; this
//! tree (~2^74) declines to the generic path.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = r#"
function run(a: number[]): number {
  const x = 2097152;
  const y = 4194304;
  const z = 2097152;
  let s = 0.0;
  for (let k = 0; k < 1; k++) {
    s = s * 1.0 + a[x * y * (z + k)];
  }
  return s;
}
const a: number[] = [];
for (let i = 0; i < 64; i++) a.push(7.5 + i);
console.log("s:" + run(a));
"#;

/// The wrapped read must not happen: node's answer is NaN (the index is far
/// out of bounds in double arithmetic), and the wrap would print `s:7.5` —
/// element 0, in bounds, wrong.
#[test]
fn a_multiply_chain_that_wraps_i64_declines_to_the_generic_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, SOURCE).expect("write entry");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    for moving_gc in [false, true] {
        let mut command = Command::new(&output);
        command.current_dir(dir.path());
        if moving_gc {
            command
                .env("PERRY_GC_FORCE_EVACUATE", "1")
                .env("PERRY_GC_VERIFY_EVACUATION", "1");
        }
        let run = command.output().expect("run binary");
        assert!(run.status.success());
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "s:NaN\n",
            "a wrapped affine index read an in-bounds element the generic path \
             never touches (moving_gc={moving_gc})"
        );
    }
}
