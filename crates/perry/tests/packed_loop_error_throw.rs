//! `throw new Error(…)` and other constructed operands inside a packed counted
//! loop (#9151, #9232).
//!
//! Admitting these depends on #9210's writeback, which is emitted at the throw
//! site — AFTER the operand is lowered. So an operand that can unwind ON ITS
//! OWN skips the flush and leaves the loop-carried accumulators stale, which is
//! #9185's defect one level deeper. Two things can unwind that way and neither
//! looks like a call at the syntax level:
//!
//!   * `new Error(msg)` stringifies `msg`, dispatching to a user `toString`.
//!   * `a + b` coerces, dispatching to a user `valueOf`/`toString`.
//!
//! Both are admitted only when the operand is provably coercion-free. The
//! rejection tests below are the load-bearing half of this file: each one
//! returned `c=0 s=0` against node's `c=40 s=780` while the predicate was
//! merely "is this loop-safe", and each is a silent wrong answer rather than a
//! missed optimisation.
//!
//! The `Error`-shadowing tests guard the other half. Accepting the Error nodes
//! is sound only because HIR emits them for the INTRINSIC constructor alone
//! (`lower_new` gates on `!shadowed_by_user_binding`); a user `class Error`
//! whose constructor shrinks the array would otherwise run against a hoisted
//! length. Each shadow case mutates the array MID-LOOP and reports the
//! iteration count, so a wrongly hoisted length reads `iters=10`, not `iters=3`.

use std::path::PathBuf;
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

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
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run: Output = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).trim().to_owned()
}

const PRELUDE: &str = r#"
const arr: number[] = [];
for (let i = 0; i < 64; i++) arr.push(i);
"#;

const SHADOW_BODY: &str = r#"
function f(): string {
  let n = 0; let s = 0;
  for (let i = 0; i < arr.length; i++) {
    if (i === 1) { const e = new Error("shrink"); if (n < 0) throw e; }
    n++; const v = arr[i]; s += (v === undefined ? 0 : v);
  }
  return "iters=" + n + " sum=" + s + " len=" + arr.length;
}
console.log(f());
"#;

// --- the operand must not be able to unwind before the flush -----------------

#[test]
fn an_error_message_whose_tostring_throws_keeps_the_accumulators() {
    // Gave `c=0 s=0`: the toString unwound during Error construction, before
    // the writeback at the throw site could run.
    let out = compile_and_run(&format!(
        "{PRELUDE}
        const evil: any = {{ toString(): string {{ throw new Error(\"from toString\"); }} }};
        function f(): string {{
            let s = 0; let c = 0;
            try {{
                for (let i = 0; i < arr.length; i++) {{
                    if (arr[i] === 40) throw new Error(evil);
                    c++; s += arr[i];
                }}
            }} catch (e: any) {{ return \"c=\" + c + \" s=\" + s + \" msg=\" + e.message; }}
            return \"none\";
        }}
        console.log(f());
        "
    ));
    assert_eq!(out, "c=40 s=780 msg=from toString");
}

#[test]
fn a_binary_operand_whose_valueof_throws_keeps_the_accumulators() {
    // No Error construction at all — plain `+` dispatching to a user valueOf.
    // Gave `c=0 s=0`.
    let out = compile_and_run(&format!(
        "{PRELUDE}
        const evil: any = {{ valueOf(): number {{ throw new Error(\"from valueOf\"); }} }};
        const two: any = 2;
        function f(): string {{
            let s = 0; let c = 0;
            try {{
                for (let i = 0; i < arr.length; i++) {{
                    if (arr[i] === 40) throw (evil + two);
                    c++; s += arr[i];
                }}
            }} catch (e: any) {{ return \"c=\" + c + \" s=\" + s + \" msg=\" + (e.message || e); }}
            return \"none\";
        }}
        console.log(f());
        "
    ));
    assert_eq!(out, "c=40 s=780 msg=from valueOf");
}

#[test]
fn a_unary_operand_whose_valueof_throws_keeps_the_accumulators() {
    let out = compile_and_run(&format!(
        "{PRELUDE}
        const evil: any = {{ valueOf(): number {{ throw new Error(\"unary valueOf\"); }} }};
        function f(): string {{
            let s = 0; let c = 0;
            try {{
                for (let i = 0; i < arr.length; i++) {{
                    if (arr[i] === 40) throw (-evil);
                    c++; s += arr[i];
                }}
            }} catch (e: any) {{ return \"c=\" + c + \" s=\" + s + \" msg=\" + (e.message || e); }}
            return \"none\";
        }}
        console.log(f());
        "
    ));
    assert_eq!(out, "c=40 s=780 msg=unary valueOf");
}

#[test]
fn a_getter_that_throws_in_the_message_keeps_the_accumulators() {
    let out = compile_and_run(&format!(
        "{PRELUDE}
        const holder: any = {{ get boom(): string {{ throw new Error(\"getter\"); }} }};
        function f(): string {{
            let s = 0; let c = 0;
            try {{
                for (let i = 0; i < arr.length; i++) {{
                    if (arr[i] === 40) throw new Error(holder.boom);
                    c++; s += arr[i];
                }}
            }} catch (e: any) {{ return \"c=\" + c + \" s=\" + s + \" msg=\" + e.message; }}
            return \"none\";
        }}
        console.log(f());
        "
    ));
    assert_eq!(out, "c=40 s=780 msg=getter");
}

#[test]
fn throwing_an_object_as_is_never_coerces_it() {
    // The counterpart to the rejections above: thrown AS-IS, nothing converts
    // it, so this stays on the fast path and the toString must never run.
    let out = compile_and_run(&format!(
        "{PRELUDE}
        const evil: any = {{ toString(): string {{ throw new Error(\"should not run\"); }} }};
        function f(): string {{
            let s = 0; let c = 0;
            try {{
                for (let i = 0; i < arr.length; i++) {{
                    if (arr[i] === 40) throw evil;
                    c++; s += arr[i];
                }}
            }} catch (e: any) {{ return \"c=\" + c + \" s=\" + s + \" isObj=\" + (typeof e); }}
            return \"none\";
        }}
        console.log(f());
        "
    ));
    assert_eq!(out, "c=40 s=780 isObj=object");
}

// --- the shapes that must stay FAST -----------------------------------------

#[test]
fn an_intrinsic_error_throw_keeps_the_packed_clone() {
    let out = compile_and_run(&format!(
        r#"{PRELUDE}
function f(): number {{
  let s = 0;
  for (let i = 0; i < arr.length; i++) {{
    if (arr[i] === 999) throw new Error("unreachable");
    s += arr[i];
  }}
  return s;
}}
console.log(f());
"#
    ));
    assert_eq!(out, "2016");
}

#[test]
fn an_intrinsic_error_throw_is_taken_at_the_right_index() {
    let out = compile_and_run(&format!(
        r#"{PRELUDE}
function f(): string {{
  let s = 0;
  try {{
    for (let i = 0; i < arr.length; i++) {{
      if (arr[i] === 40) throw new Error("hit@" + i);
      s += arr[i];
    }}
  }} catch (e: any) {{ return "caught:" + e.message + " partial=" + s; }}
  return "none:" + s;
}}
console.log(f());
"#
    ));
    assert_eq!(out, "caught:hit@40 partial=780");
}

#[test]
fn an_array_element_in_the_message_is_coercion_free() {
    // `arr[i]` is a genuine double by the clone's own guard, so stringifying it
    // is builtin.
    let out = compile_and_run(&format!(
        r#"{PRELUDE}
function f(): string {{
  let s = 0;
  try {{
    for (let i = 0; i < arr.length; i++) {{
      if (arr[i] === 40) throw new Error("v=" + arr[i]);
      s += arr[i];
    }}
  }} catch (e: any) {{ return "s=" + s + " msg=" + e.message; }}
  return "none";
}}
console.log(f());
"#
    ));
    assert_eq!(out, "s=780 msg=v=40");
}

#[test]
fn native_error_subclasses_are_accepted_too() {
    let out = compile_and_run(&format!(
        r#"{PRELUDE}
function f(kind: number): number {{
  let s = 0;
  for (let i = 0; i < arr.length; i++) {{
    if (arr[i] === 999) {{
      if (kind === 0) throw new TypeError("t");
      if (kind === 1) throw new RangeError("r");
      if (kind === 2) throw new SyntaxError("s");
      throw new ReferenceError("f");
    }}
    s += arr[i];
  }}
  return s;
}}
console.log(f(0) + "," + f(1) + "," + f(2) + "," + f(3));
"#
    ));
    assert_eq!(out, "2016,2016,2016,2016");
}

#[test]
fn a_message_expression_that_shrinks_the_array_is_still_rejected() {
    let out = compile_and_run(
        r#"
const arr: number[] = [1,2,3,4,5,6,7,8,9,10];
function shrink(): string { arr.length = 3; return "boom"; }
function f(): string {
  let n = 0; let s = 0;
  for (let i = 0; i < arr.length; i++) {
    if (i === 1) { const e = new Error(shrink()); if (n < 0) throw e; }
    n++; const v = arr[i]; s += (v === undefined ? 0 : v);
  }
  return "iters=" + n + " sum=" + s + " len=" + arr.length;
}
console.log(f());
"#,
    );
    assert_eq!(out, "iters=3 sum=6 len=3");
}

// --- `Error` must be the intrinsic, not a look-alike ------------------------

#[test]
fn a_user_class_named_error_is_not_the_intrinsic() {
    let out = compile_and_run(&format!(
        r#"
const arr: number[] = [1,2,3,4,5,6,7,8,9,10];
class Error {{ constructor(_m: string) {{ arr.length = 3; }} }}
{SHADOW_BODY}"#
    ));
    assert_eq!(out, "iters=3 sum=6 len=3");
}

#[test]
fn a_const_function_named_error_is_not_the_intrinsic() {
    let out = compile_and_run(&format!(
        r#"
const arr: number[] = [1,2,3,4,5,6,7,8,9,10];
const Error: any = function (this: any, _m: string) {{ arr.length = 3; }};
{SHADOW_BODY}"#
    ));
    assert_eq!(out, "iters=3 sum=6 len=3");
}

#[test]
fn a_function_valued_error_binding_is_not_the_intrinsic() {
    // The shadow's identity is not visible in the initializer at all.
    let out = compile_and_run(&format!(
        r#"
const arr: number[] = [1,2,3,4,5,6,7,8,9,10];
function makeErr(): any {{ return function (this: any, _m: string) {{ arr.length = 3; }}; }}
const Error: any = makeErr();
{SHADOW_BODY}"#
    ));
    assert_eq!(out, "iters=3 sum=6 len=3");
}

#[test]
fn a_block_scoped_class_named_error_is_not_the_intrinsic() {
    let out = compile_and_run(
        r#"
const arr: number[] = [1,2,3,4,5,6,7,8,9,10];
function f(): string {
  class Error { constructor(_m: string) { arr.length = 3; } }
  let n = 0; let s = 0;
  for (let i = 0; i < arr.length; i++) {
    if (i === 1) { const e = new Error("shrink"); if (n < 0) throw e; }
    n++; const v = arr[i]; s += (v === undefined ? 0 : v);
  }
  return "iters=" + n + " sum=" + s + " len=" + arr.length;
}
console.log(f());
"#,
    );
    assert_eq!(out, "iters=3 sum=6 len=3");
}
