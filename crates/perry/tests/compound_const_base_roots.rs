//! A statement-form compound member assignment through a `const` binding
//! (`o.x += e`) must not root a copy of the receiver.
//!
//! `hoist_compound_member_assign` evaluates the base once by spilling it into
//! a `__cmpd_base_N` temp. For a `const` binding the spill is redundant, since
//! nothing can reassign the binding between the read and the write, and it
//! was not free. The temp is a second pointer-typed local, so every compound
//! assignment paid a string-addref test, a root-slot store and an
//! incremental-mark root-shading gate (`PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT`)
//! for an object the binding's own slot already roots. On n-body that was
//! 6 of the 9 gates per body pair.
//!
//! The test compiles one module twice over, a `const` receiver and a `let`
//! receiver with otherwise identical bodies, and counts the gates in each
//! function's emitted IR. The `let` receiver must keep its snapshot (its RHS
//! may reassign it), so it is the control: if the counts are equal, either
//! the fix is gone or the control stopped measuring anything.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            perry_bin()
                .parent()
                .expect("compiler directory")
                .to_path_buf()
        })
}

const SOURCE: &str = r#"
class Cell { x = 0; label = ""; }
function bumpConst(n: number): number {
  const o = new Cell();
  for (let i = 0; i < n; i++) { o.x += i; o.label += i % 50 === 0 ? "c" : ""; }
  return o.x + o.label.length;
}
function bumpLet(n: number): number {
  let o = new Cell();
  for (let i = 0; i < n; i++) { o.x += i; o.label += i % 50 === 0 ? "c" : ""; }
  return o.x + o.label.length;
}
console.log("bump", bumpConst(100), bumpLet(100));
"#;

/// Every function body in `ir` whose symbol contains `name`, concatenated.
fn bodies<'a>(ir: &'a str, name: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut rest = ir;
    while let Some(start) = rest.find("\ndefine ") {
        let body = &rest[start + 1..];
        let end = body.find("\n}\n").map(|e| e + 2).unwrap_or(body.len());
        let header = body.lines().next().unwrap_or("");
        if header.contains(name) {
            out.push(&body[..end]);
        }
        rest = &body[end..];
    }
    out
}

fn gates(bodies: &[&str]) -> usize {
    bodies
        .iter()
        .map(|b| {
            b.matches("@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT")
                .count()
        })
        .sum()
}

#[test]
fn const_receiver_compound_assign_roots_no_receiver_copy() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("cmpd_roots.ts");
    std::fs::write(&src, SOURCE).expect("write source");
    let ll_dir = dir.path().join("ll");
    std::fs::create_dir_all(&ll_dir).expect("ll dir");
    let exe = dir.path().join("cmpd_roots");

    let out = Command::new(perry_bin())
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .arg("--no-cache")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .env("PERRY_SAVE_LL", &ll_dir)
        .current_dir(dir.path())
        .output()
        .expect("run perry");
    assert!(
        out.status.success(),
        "perry compile failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let run = Command::new(&exe).output().expect("run binary");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "bump 4952 4952\n");

    let ir: String = std::fs::read_dir(&ll_dir)
        .expect("saved IR")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "ll"))
        .map(|e| std::fs::read_to_string(e.path()).expect("read IR"))
        .collect();
    let const_bodies = bodies(&ir, "__bumpConst");
    let let_bodies = bodies(&ir, "__bumpLet");
    assert!(
        !const_bodies.is_empty() && const_bodies.len() == let_bodies.len(),
        "expected matching bumpConst/bumpLet bodies, got {} and {}",
        const_bodies.len(),
        let_bodies.len()
    );
    let (c, l) = (gates(&const_bodies), gates(&let_bodies));
    // Two compound assignments per body; the `let` receiver roots a snapshot
    // for each, the `const` receiver for neither.
    assert!(
        l >= c + 2 * let_bodies.len(),
        "a const receiver must root no copy: {c} gates for const vs {l} for let \
         over {} bodies",
        let_bodies.len()
    );
}
