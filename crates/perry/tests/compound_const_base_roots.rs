//! Immutable receivers need no compound-assignment snapshot.
//!
//! Check the lowering directly: layout admission and root shading add LLVM
//! gates of their own, so their aggregate count does not count receiver copies.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
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

#[test]
fn const_receiver_compound_assign_needs_no_receiver_snapshot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("cmpd_roots.ts");
    let exe = dir.path().join("cmpd_roots");
    std::fs::write(&src, SOURCE).expect("write source");
    let out = Command::new(perry_bin())
        .args(["compile", "--print-hir", "--no-auto-optimize", "--no-cache"])
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .current_dir(dir.path())
        .output()
        .expect("run perry");
    assert!(
        out.status.success(),
        "compile failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let hir = String::from_utf8(out.stdout).expect("HIR");
    let const_body = hir
        .split("  - bumpConst ")
        .nth(1)
        .expect("bumpConst")
        .split("  - bumpLet ")
        .next()
        .unwrap();
    let let_body = hir
        .split("  - bumpLet ")
        .nth(1)
        .expect("bumpLet")
        .split("\nClasses:")
        .next()
        .unwrap();
    assert!(!const_body.contains("__cmpd_base_"), "{const_body}");
    assert_eq!(let_body.matches("__cmpd_base_").count(), 2, "{let_body}");
    let run = Command::new(&exe).output().expect("run binary");
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "bump 4952 4952\n");
}
