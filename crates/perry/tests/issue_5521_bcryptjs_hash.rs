//! Regression test for #5521: two codegen bugs that together broke
//! bcryptjs's Blowfish core (`hashSync` returned `undefined`, then the
//! cipher computed a wrong digest).
//!
//! 1. **Forward-referenced callee not arg-padded.** HIR call-site padding
//!    pads missing trailing args only when the callee's signature is already
//!    registered, so a call to a function defined *later* in the module read
//!    uninitialized arg registers (a stray `0`) for the skipped params.
//!    bcryptjs: `hashSync` calls the later `_hash`, whose `callback` param
//!    read as `0` → `typeof callback === 'number'` → async branch → returned
//!    `undefined`. Fixed by the post-lowering `fill_default_arguments` pass,
//!    which has every function's final shape and pads regardless of order.
//!
//! 2. **Captured + reassigned parameter not boxed.** A parameter captured by
//!    a (hoisted) nested closure and reassigned in the enclosing scope was
//!    never heap-boxed (the boxing analysis only considered `Stmt::Let`
//!    locals), so the hoisted closure read a stale by-value snapshot taken at
//!    function entry. bcryptjs: `_crypt(b, salt, rounds, …)` does
//!    `rounds = (1 << rounds) >>> 0` (12 → 4096) and the hoisted `next()`
//!    closure captures `rounds`; unboxed, `next()` saw 12, so the key
//!    schedule ran 12 iterations instead of 4096 → wrong hash. Fixed by
//!    `collect_boxed_param_ids` boxing captured+mutated params.
//!
//! This test exercises both bugs directly (no bcryptjs dependency) so it
//! runs in CI without `node_modules`: the forward-ref arity shape and the
//! captured-reassigned-parameter shape, including the exact
//! `(1 << rounds) >>> 0` reassignment bcryptjs uses.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn bcryptjs_hash_core_codegen_shapes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");

    std::fs::write(
        &entry,
        r#"
// Bug 1: a forward-referenced callee must still have its missing trailing
// args padded to `undefined`. `sync` is lowered before `inner` is defined.
function sync(a, b) {
  return inner(a, b);
}
function inner(a, b, callback, prog) {
  // `callback`/`prog` were never passed → must read as `undefined`, not 0.
  return (typeof callback) + "," + (typeof prog);
}
console.log("A=" + sync("p", "s"));

// Bug 2: a parameter reassigned in the enclosing scope and captured by a
// hoisted function declaration must share a boxed cell (closure sees the
// reassigned value, not the entry snapshot). This is bcryptjs `_crypt`'s
// exact `rounds = (1 << rounds) >>> 0` shape.
function crypt(rounds) {
  rounds = (1 << rounds) >>> 0;     // 12 -> 4096
  function next() { return rounds; } // hoisted decl capturing `rounds`
  return next();
}
console.log("B=" + crypt(12));

// Plain f64 reassignment captured by a hoisted decl — same boxing path.
function f(x) {
  x = x * 2;
  function g() { return x; }
  return g();
}
console.log("C=" + f(21));

// Loop-driven mutation of a captured parameter (the bcryptjs round loop
// shape): closure reads the live value across iterations.
function loop(n) {
  var acc = 0;
  function step() { acc = acc + n; }
  for (var i = 0; i < 4; i++) step();
  return acc;
}
console.log("D=" + loop(10));
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout, "A=undefined,undefined\nB=4096\nC=42\nD=40\n",
        "forward-ref arg padding (#5521 bug 1) and captured-reassigned-param \
         boxing (#5521 bug 2) must both hold"
    );
}
