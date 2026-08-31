//! Regression for #8968: a compound or logical assignment to a PRIVATE member
//! read the wrong slot, and answered `undefined` every time.
//!
//! `lower_assign_target_to_expr` builds the READ half of `a op= b`. Its
//! private-name arm addressed the field by its SOURCE spelling
//! (`PropertyGet { property: "#x" }`) while every other private access — the
//! ordinary read in `expr_member/member_tail.rs` and the write half in
//! `lower_expr_assignment` — addresses it by the MANGLED storage key
//! `private_storage_property` produces (`#<perry:private-value:{id}:#x>`). So
//! the read missed, silently:
//!
//!   this.#n += 1     // NaN
//!   this.#s += "b"   // "undefinedb"
//!   this.#v ||= d    // ALWAYS stored d
//!   this.#v ??= d    // ALWAYS stored d
//!   this.#v &&= d    // NEVER stored
//!
//! The report was hono answering an unmatched route with `200 ""` and never
//! invoking a registered `app.notFound()` handler. hono's `Context` memoizes
//! its response as `get res() { return this.#res ||= new Response(null, …) }`,
//! so every read of `c.res` threw away the finalized response — including the
//! 404 the not-found handler had already produced — and replaced it with a
//! fresh empty 200. A matched route never reads `c.res` (the single-handler
//! fast path returns the handler's response directly), which is exactly why
//! only the miss path was visibly wrong.
//!
//! Nothing here asserts on an error: every case is a WRONG ANSWER, so each
//! test pins the value, not the absence of a throw.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile `source` as a single entry module and return its stdout.
fn run_source(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let entry = root.join("main.ts");
    std::fs::write(&entry, source).expect("write source");
    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
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
    let run = Command::new(&output).output().expect("run binary");
    assert!(
        run.status.success(),
        "binary failed (status {:?})\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).trim().to_string()
}

/// `||=` and `??=` must SHORT-CIRCUIT on a private field that already holds a
/// truthy / non-nullish value — no store, and the existing value is the result.
/// `&&=` is the mirror: it must store there, and skip on a falsy value.
#[test]
fn logical_assignment_reads_the_private_field_it_writes() {
    let stdout = run_source(
        r#"class Box {
  #or: any;
  #and: any;
  #qq: any;
  set(a: any, b: any, c: any): void { this.#or = a; this.#and = b; this.#qq = c; }
  ops(): string {
    const r1 = this.#or ||= "OR";
    const r2 = this.#and &&= "AND";
    const r3 = this.#qq ??= "QQ";
    return JSON.stringify([r1, r2, r3]);
  }
  peek(): string { return JSON.stringify([this.#or, this.#and, this.#qq]); }
}
function probe(a: any, b: any, c: any): void {
  const box = new Box();
  box.set(a, b, c);
  console.log(box.ops() + " " + box.peek());
}
probe("T", "T", "T");
probe(0, 0, 0);
probe(undefined, undefined, undefined);
"#,
    );
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec![
            // truthy / truthy / non-nullish: only `&&=` stores.
            r#"["T","AND","T"] ["T","AND","T"]"#,
            // falsy 0: `||=` stores, `&&=` short-circuits to 0, `??=` keeps 0
            // (0 is not nullish).
            r#"["OR",0,0] ["OR",0,0]"#,
            // undefined: `||=` and `??=` store, `&&=` short-circuits.
            r#"["OR",null,"QQ"] ["OR",null,"QQ"]"#,
        ]
    );
}

/// The arithmetic and string compound operators read the same slot. `+=` on a
/// number gave NaN and on a string gave `"undefined…"` — the clearest evidence
/// that the read, not the short-circuit, was what broke.
#[test]
fn arithmetic_compound_assignment_reads_the_private_field() {
    let stdout = run_source(
        r#"class Counter {
  #n = 10;
  #s = "a";
  run(): string {
    this.#n += 1;
    this.#n *= 2;
    this.#n -= 2;
    this.#s += "b";
    return this.#n + " " + this.#s;
  }
}
console.log(new Counter().run());
"#,
    );
    assert_eq!(stdout, "20 ab");
}

/// A STATIC private field goes through the same lowering.
#[test]
fn static_private_field_compound_assignment_reads_the_field() {
    let stdout = run_source(
        r#"class Acc {
  static #total = 5;
  static bump(): number { Acc.#total += 3; return Acc.#total; }
}
console.log(Acc.bump() + " " + Acc.bump());
"#,
    );
    assert_eq!(stdout, "8 11");
}

/// The exact hono shape: a private field memoized behind a getter with `||=`.
/// Reading the getter twice must yield the SAME object, and must not overwrite
/// a value the setter had already installed.
#[test]
fn private_field_memoized_by_a_getter_is_computed_once() {
    let stdout = run_source(
        r#"let built = 0;
class Ctx {
  #res: any;
  get res(): any {
    return this.#res ||= { tag: "fresh-" + (++built) };
  }
  set res(v: any) { this.#res = v; }
}
const c = new Ctx();
c.res = { tag: "finalized" };
const first = c.res;
const second = c.res;
console.log(first.tag + " " + second.tag + " " + (first === second) + " built=" + built);
const d = new Ctx();
console.log(d.res.tag + " " + d.res.tag + " built=" + built);
"#,
    );
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec![
            "finalized finalized true built=0",
            "fresh-1 fresh-1 built=1",
        ]
    );
}

/// A private ACCESSOR pair must run its getter for the read half and its setter
/// for the write half — the guard the fix installs is what carries the runtime
/// access hint that resolves an accessor by its mangled name.
#[test]
fn private_accessor_compound_assignment_runs_getter_and_setter() {
    let stdout = run_source(
        r#"class G {
  #raw = 7;
  #gets = 0;
  #sets = 0;
  get #val(): any { this.#gets += 1; return this.#raw; }
  set #val(v: any) { this.#sets += 1; this.#raw = v; }
  run(): string {
    this.#val += 1;
    this.#val ||= 99;
    return this.#raw + " gets=" + this.#gets + " sets=" + this.#sets;
  }
}
console.log(new G().run());
"#,
    );
    // `+= 1` is one get + one set; `||= 99` short-circuits on the truthy 8, so
    // it is one more get and NO set.
    assert_eq!(stdout, "8 gets=2 sets=1");
}

/// The read half is now brand-guarded like every other private access, so a
/// compound assignment against a receiver whose class did not declare the
/// member throws `TypeError` instead of silently reading `undefined` and
/// writing an ordinary `"#x"` string property onto the stranger.
#[test]
fn compound_assignment_on_a_foreign_receiver_throws() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"class Holder {
  #n = 1;
  static bump(target: any): void { target.#n += 1; }
}
try {
  Holder.bump({});
  console.log("no throw");
} catch (e: any) {
  console.log("threw: " + e.message);
}
"#,
    )
    .expect("write source");
    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
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
    let run = Command::new(&output).output().expect("run binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stdout.contains("threw:") || stderr.contains("private member"),
        "expected a TypeError for the foreign receiver\nstatus: {:?}\n\
         stdout:\n{stdout}\nstderr:\n{stderr}",
        run.status.code()
    );
}

/// A PUBLIC field never had the bug — it is the control that shows the fix is
/// scoped to the private-name arm.
#[test]
fn public_field_compound_assignment_is_unchanged() {
    let stdout = run_source(
        r#"class P {
  v: any = "KEPT";
  n = 10;
  run(): string { this.v ||= "FRESH"; this.n += 5; return this.v + " " + this.n; }
}
console.log(new P().run());
"#,
    );
    assert_eq!(stdout, "KEPT 15");
}
