//! Regression test: a destructuring `var { … } = factory()` declared AFTER a
//! sibling closure (function OR class method) that captures one of its bindings
//! must let that closure read the value the destructuring assigns — not
//! `undefined`.
//!
//! This is the semver wall in the claude-code bundle. esbuild's `__commonJS`
//! memoizer (`p=(cb,mod)=>()=>(mod||cb((mod={exports:{}}).exports,mod),mod.exports)`)
//! wraps semver's `re` module; consumer modules do
//!
//!     var { safeRe: QSq, t: dSq } = bV6();   // bV6 = the memoized `re` factory
//!
//! and `Comparator.parse` (a class method declared ABOVE that `var`) reads
//! `QSq[dSq.COMPARATOR]`. Perry threw `TypeError: Cannot read properties of
//! undefined (reading 'COMPARATOR')` — `dSq` read undefined inside the closure.
//!
//! Root cause: the function-body `var`-hoist pre-passes
//! (`predefine_var_bindings_in_function_body` /
//! `pre_register_forward_captured_lets` / the `lower_fn_expr` inline pre-pass)
//! box the forward-captured `var` slot, but the destructuring-leaf lowering in
//! `destructuring/pattern_binding.rs` allocated a FRESH local for `dSq` instead
//! of reusing the pre-hoisted boxed id. The `=factory()` assignment then landed
//! in a different slot than the box the closure captured, so the closure read
//! the never-written box (undefined).
//!
//! Fix: the `Pat::Ident` leaf in `lower_pattern_binding_into` now reuses a
//! `var`-hoisted binding of the same name (mirroring the plain `Pat::Ident`
//! var-decl reuse in `destructuring/var_decl.rs`); the `lower_fn_expr` inline
//! pre-pass also walks destructuring patterns (not just `Pat::Ident`) so the
//! function-expression factory body pre-registers the destructured bindings.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn destructured_var_after_capturing_closure_reads_assigned_value() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
// esbuild __commonJS lazy memoizer, verbatim shape from the claude-code bundle.
const p = (cb: any, mod?: any) => () =>
  (mod || cb((mod = { exports: {} }).exports, mod), mod.exports);

// semver `re` module: reassigns `exports` to a fresh object, then populates it.
const bV6 = p((QQ: any, pRq: any) => {
  QQ = pRq.exports = {};
  const AK: any = (QQ.t = {});
  const yt9: any = (QQ.safeRe = []);
  let ht9 = 0;
  const n9 = (name: string) => { AK[name] = ht9; yt9[ht9] = "rx"; ht9++; };
  n9("COMPARATOR");
  n9("COMPARATORLOOSE");
});

// Consumer module: the capturing CLASS is declared ABOVE the destructuring
// `var`, exactly like semver's Comparator + `var {safeRe:QSq,t:dSq}=bV6()`.
const consumer = p((nCO: any, nSq: any) => {
  class Comparator {
    options: any;
    constructor(o: any) { this.options = o; }
    parse(): any {
      return this.options.loose ? QSq[dSq.COMPARATORLOOSE] : QSq[dSq.COMPARATOR];
    }
  }
  nSq.exports = Comparator;
  // eslint-disable-next-line no-var
  var { safeRe: QSq, t: dSq } = bV6();
});

// Class is instantiated by a later caller (the real semver flow), and a plain
// FUNCTION closure capturing the same destructured var (the non-class trigger).
const C = consumer();
const c = new C({ loose: false });
console.log("class:", c.parse());

function fnCapture() {
  function read() { return dSq2.COMPARATOR; }
  var { t: dSq2 } = bV6();
  return read();
}
console.log("fn:", fnCapture());
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .env("PERRY_ALLOW_PERRY_FEATURES", "1")
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
        "compiled binary failed (the semver `dSq.COMPARATOR` undefined throw)\n\
         status: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout, "class: rx\nfn: 0\n",
        "a destructured `var` declared after a capturing closure must let that \
         closure read the assigned value (node prints the same)"
    );
}
