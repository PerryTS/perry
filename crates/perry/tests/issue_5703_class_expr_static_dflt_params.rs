//! Regression test for #5703 — a STATIC method of a class EXPRESSION
//! (`var C = class { static m(...) {...} }`) called with fewer arguments than
//! declared dropped its default-parameter / array-destructuring prologue.
//!
//! Root cause: `C.m()` on a class-expression value reaches the fused
//! get-static-method+call path in
//! `lower_call/property_get/static_dispatch.rs` (class DECLARATIONS instead
//! lower `C.m()` to a `StaticMethodCall`, which already pads — #235). That
//! path forwarded only the supplied args to a fixed-arity LLVM function, so
//! missing slots arrived as an uninitialized `0.0` register rather than
//! `undefined`. The callee's default-param prologue (`if (p === undefined) p =
//! …`) never fired, and array destructuring of the missing slot
//! (`GetIterator(p)`) threw `TypeError: is not iterable`. This broke ~61
//! `language/expressions/class` test262 cases (static methods of class
//! expressions with default params / destructuring), dominated by static
//! async-generator methods with an array-destructuring default param.
//!
//! Fix: pad the missing positional slots with `undefined` in the static
//! dispatch path, mirroring the `StaticMethodCall` path. Expected outputs are
//! byte-for-byte what `node --experimental-strip-types` prints.

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
        .arg("--no-cache")
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
        "compiled binary failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// Scalar default param on a class-expression static method, called with no
/// args. Pre-fix this printed `0` (uninitialized register), then `9`.
#[test]
fn class_expr_static_scalar_default_param() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
var C = class {
  static m(x = 5) { return x; }
};
console.log(C.m());
console.log(C.m(9));
"#,
    );
    assert_eq!(
        stdout, "5\n9\n",
        "missing arg must default to 5 (undefined-padded), supplied arg wins"
    );
}

/// Array-destructuring default param — the dominant #5703 signature. Pre-fix
/// `C.m()` threw `TypeError: is not iterable` (the missing slot was `0.0`, not
/// the `undefined` that triggers the default).
#[test]
fn class_expr_static_destructuring_default_param() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
var C = class {
  static m([x, y] = [3, 4]) { return x + y; }
};
console.log(C.m());
console.log(C.m([1, 2]));
"#,
    );
    assert_eq!(
        stdout, "7\n3\n",
        "default array destructured when no arg supplied"
    );
}

/// A leading non-default param BEFORE the synthesized `arguments` slot
/// (`static method(x, _ = 0) { … arguments … }`) on a class expression. The
/// synth-`arguments` dispatch branch previously pushed only the arguments
/// object, so `x` received the (empty) arguments array — printing `x=` instead
/// of `x=undefined` (test262 `params-dflt-meth-static-args-unmapped`). The
/// `arguments` object must hold all passed args, independent of the (unmapped)
/// named params.
#[test]
fn class_expr_static_leading_param_with_arguments() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
var C = class {
  static method(x, _ = 0) {
    console.log("x=" + x + " _=" + _);
    console.log(
      "a0=" + arguments[0] + " a1=" + arguments[1] + " len=" + arguments.length
    );
  }
};
C.method();
C.method(7, 8, 9);
"#,
    );
    assert_eq!(
        stdout, "x=undefined _=0\na0=undefined a1=undefined len=0\nx=7 _=8\na0=7 a1=8 len=3\n",
        "leading param + synth arguments must match node (byte-for-byte)"
    );
}

/// A class-expression static method whose ONLY parameter is the synthesized
/// `arguments` (effect-style `static pipe() { … arguments.length … }`) must be
/// unaffected by the leading-param fix above — guards the fixed_count == 0 path.
#[test]
fn class_expr_static_arguments_only_unregressed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
var C = class {
  static pipe() { return arguments.length; }
};
console.log(C.pipe());
console.log(C.pipe(1, 2, 3));
"#,
    );
    assert_eq!(
        stdout, "0\n3\n",
        "arguments-only static method counts all args"
    );
}

/// Static async-generator method of a class expression with an
/// array-destructuring default param — the exact shape of the ~42
/// `async-gen-meth-static-dflt-ary-*` test262 cases.
#[test]
fn class_expr_static_async_gen_destructuring_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
var C = class {
  static async *agen([a, b] = [10, 20]) {
    yield a;
    yield b;
  }
};
(async () => {
  const out: number[] = [];
  for await (const v of C.agen()) out.push(v);
  console.log(out.join(","));
})();
"#,
    );
    assert_eq!(
        stdout, "10,20\n",
        "default applies inside a static async generator"
    );
}
