//! Regression tests: well-known-symbol methods on class *expressions*.
//!
//! `lower_class_from_ast` (the class-expression path) only special-cased
//! `[Symbol.iterator]` and `[util.inspect.custom]` computed keys; every other
//! well-known-symbol method — `[Symbol.asyncIterator]`, `[Symbol.toPrimitive]`,
//! `[Symbol.dispose]` / `[Symbol.asyncDispose]`, `static [Symbol.hasInstance]`,
//! `get [Symbol.toStringTag]` — fell through `_ => continue` and was silently
//! dropped. The same methods on a class *declaration* worked
//! (`lower_class_decl` handles them all).
//!
//! Canonical failure: an SDK-style SSE stream wrapper in a large
//! esbuild-bundled CLI app,
//!   `oV = class oV { constructor(it) { this.iterator = it }
//!                    [Symbol.asyncIterator]() { return this.iterator() } }`
//! — `for await (const ev of stream)` resolved no `@@asyncIterator`, fell
//! through to the sync-iterator path, found no `.next`, and threw
//! `TypeError: value is not iterable`.
//!
//! Fix: both lowering paths route computed well-known-symbol keys through the
//! shared `lower_well_known_computed_method` helper (lower_decl/helpers.rs).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
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

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// `[Symbol.asyncIterator]() {}` on class expressions: `for await` over an
/// instance must find `@@asyncIterator` (pre-fix: `TypeError: value is not
/// iterable`), a dynamic `obj[Symbol.asyncIterator]` read must see a function
/// (pre-fix: `undefined`), and a manual invoke must drive the iterator.
#[test]
fn class_expression_symbol_async_iterator_for_await() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function* gen() { yield 1; yield 2; }
// anonymous class expression
const A: any = class { [Symbol.asyncIterator]() { return gen(); } };
// SDK-style named class expression assigned to a same-named outer binding,
// delegating to a stored iterator factory (the esbuild-bundle shape).
var oV: any;
oV = class oV {
  iterator: any;
  constructor(it: any) { this.iterator = it; }
  [Symbol.asyncIterator]() { return this.iterator(); }
};
async function* letters() { yield "a"; yield "b"; }
(async () => {
  const got: any[] = [];
  for await (const x of new A()) got.push(x);
  console.log("A:", got.join(","));
  console.log("A dyn:", typeof new A()[Symbol.asyncIterator]);
  const s = new oV(letters);
  const evs: any[] = [];
  for await (const ev of s) evs.push(ev);
  console.log("oV:", evs.join(","));
  const it = new oV(letters)[Symbol.asyncIterator]();
  const first = await it.next();
  console.log("manual:", first.value, first.done);
})();
"#,
    );
    assert_eq!(
        stdout,
        "A: 1,2\nA dyn: function\noV: a,b\nmanual: a false\n"
    );
}

/// `[Symbol.toPrimitive](hint) {}` on a class expression: numeric and
/// default-hint coercions must invoke it (pre-fix: dropped, so instances
/// coerced to `NaN` / `[object Object]`).
#[test]
fn class_expression_symbol_to_primitive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const P: any = class {
  [Symbol.toPrimitive](hint: string) { return hint === "number" ? 42 : "str"; }
};
console.log(+new P());
console.log(`${new P()}`);
"#,
    );
    assert_eq!(stdout, "42\nstr\n");
}

/// `static [Symbol.hasInstance](v) {}` on a class expression: `instanceof`
/// must consult it (pre-fix: dropped).
#[test]
fn class_expression_static_symbol_has_instance() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const H: any = class { static [Symbol.hasInstance](v: any) { return v === 7; } };
console.log(7 instanceof H);
console.log(8 instanceof H);
"#,
    );
    assert_eq!(stdout, "true\nfalse\n");
}

/// `get [Symbol.toStringTag]() {}` on a class expression:
/// `Object.prototype.toString` must pick up the tag (pre-fix: dropped, so
/// `[object Object]`).
#[test]
fn class_expression_symbol_to_string_tag_getter() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const T: any = class { get [Symbol.toStringTag]() { return "Custom"; } };
console.log(Object.prototype.toString.call(new T()));
"#,
    );
    assert_eq!(stdout, "[object Custom]\n");
}

/// `[Symbol.dispose]()` on a class expression: a `using` block must invoke it
/// at scope exit (pre-fix: dropped, so nothing ran).
#[test]
fn class_expression_symbol_dispose_using_block() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const D: any = class {
  name: string;
  constructor(name: string) { this.name = name; }
  [Symbol.dispose]() { console.log("disposed", this.name); }
};
{
  using d = new D("x");
  console.log("in scope");
}
console.log("after");
"#,
    );
    assert_eq!(stdout, "in scope\ndisposed x\nafter\n");
}

/// A computed key that only EVALUATES to a well-known symbol — the minified
/// `[(gm = new WeakMap, Symbol.asyncIterator)]() {…}` comma form — can't be
/// recognized statically and flows through the generic computed-member
/// runtime registration. `js_register_class_computed_method` must alias the
/// well-known symbol onto the synthetic vtable slot (`@@asyncIterator` /
/// `@@iterator`) that GetIterator and the symbol-read binder consult, and the
/// key expression's side effect must still run.
#[test]
fn class_expression_comma_keyed_well_known_symbol_methods() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
var gm1: any, gm2: any;
var oV: any;
oV = class oV {
  iterator: any;
  constructor(q: any) { this.iterator = q; }
  [(gm1 = new WeakMap(), Symbol.asyncIterator)]() { return this.iterator(); }
};
const S: any = class {
  [(gm2 = 1, Symbol.iterator)]() {
    let d = false;
    return { next: () => d ? { done: true, value: 0 } : (d = true, { done: false, value: 5 }) };
  }
};
async function* mk() { yield "x"; yield "y"; }
(async () => {
  const evs: any[] = [];
  for await (const e of new oV(mk)) evs.push(e);
  console.log("comma-async:", evs.join(","));
  console.log("comma-sync:", [...new S()].join(","));
  console.log("side effects:", gm1 instanceof WeakMap, gm2);
})();
"#,
    );
    assert_eq!(
        stdout,
        "comma-async: x,y\ncomma-sync: 5\nside effects: true 1\n"
    );
}

/// The class-declaration forms must keep working identically after the
/// refactor onto the shared helper (decl-path regression guard).
#[test]
fn class_declaration_well_known_methods_still_work() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function* gen() { yield "d1"; }
class DeclAsync { [Symbol.asyncIterator]() { return gen(); } }
class DeclPrim { [Symbol.toPrimitive](hint: string) { return hint === "number" ? 7 : "p"; } }
class DeclHas { static [Symbol.hasInstance](v: any) { return v === 1; } }
class DeclTag { get [Symbol.toStringTag]() { return "DeclTag"; } }
(async () => {
  const got: any[] = [];
  for await (const x of new DeclAsync()) got.push(x);
  console.log(got.join(","));
  console.log(+new DeclPrim());
  console.log(1 instanceof DeclHas, 2 instanceof DeclHas);
  console.log(Object.prototype.toString.call(new DeclTag()));
})();
"#,
    );
    assert_eq!(stdout, "d1\n7\ntrue false\n[object DeclTag]\n");
}
