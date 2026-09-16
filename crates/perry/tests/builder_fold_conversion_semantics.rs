//! #10357: an implicit conversion in a folded builder value can call user
//! code. These are the observable consequences — the HIR tests in
//! `perry-hir/tests/builder_fold_conversion.rs` pin which shapes fold, this
//! file pins that the program behaves like node either way.

use std::path::PathBuf;
use std::process::Command;

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
        .current_dir(dir.path())
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

#[test]
fn a_value_of_that_reads_the_builder_sees_the_allocated_object() {
    // The #10357 repro: node prints `string:str`; the fold used to move
    // `weird.valueOf()` into `o`'s TDZ.
    let stdout = compile_and_run(
        r#"
function run(): string {
  const weird = { valueOf(): any { return o; } };
  const o: any = {};
  o.a = "" + weird;
  return typeof o.a + ":" + (o.a === "[object Object]" ? "str" : "other");
}
try { console.log(run()); } catch (e: any) { console.log("threw " + e.name); }
"#,
    );
    assert_eq!(stdout, "string:str\n");
}

#[test]
fn every_converting_form_sees_the_allocated_object() {
    let stdout = compile_and_run(
        r#"
function probe(kind: string): string {
  const seen: string[] = [];
  const w = {
    valueOf(): any { seen.push(typeof o); return 1; },
    toString(): string { seen.push(typeof o); return "w"; },
  };
  const o: any = {};
  if (kind === "neg") o.a = -(w as any);
  else if (kind === "lt") o.a = (w as any) < 2;
  else if (kind === "tpl") o.a = `${w}`;
  else o.a = "" + w;
  return kind + "=" + seen.join(",");
}
for (const kind of ["plus", "neg", "lt", "tpl"]) {
  try { console.log(probe(kind)); } catch (e: any) { console.log(kind + " threw " + e.name); }
}
"#,
    );
    assert_eq!(stdout, "plus=object\nneg=object\nlt=object\ntpl=object\n");
}

#[test]
fn a_conversion_in_a_gap_statement_sees_the_allocated_object() {
    let stdout = compile_and_run(
        r#"
function run(): string {
  const w = { toString(): string { return typeof o; } };
  const o: any = {};
  const s = `${w}`;
  o.a = s;
  return o.a;
}
try { console.log(run()); } catch (e: any) { console.log("threw " + e.name); }
"#,
    );
    assert_eq!(stdout, "object\n");
}

#[test]
fn unobserved_conversions_still_build_the_same_object() {
    let stdout = compile_and_run(
        r#"
function build(r: number, i: number): any {
  const o: any = {};
  o.a = i; o.b = r + i; o.c = `k${i}`; o.d = -r; o.e = r < i;
  return o;
}
console.log(JSON.stringify(build(3, 4)), Object.keys(build(1, 2)).join(","));
"#,
    );
    assert_eq!(
        stdout,
        "{\"a\":4,\"b\":7,\"c\":\"k4\",\"d\":-3,\"e\":true} a,b,c,d,e\n"
    );
}

#[test]
fn loop_passes_see_the_right_binding() {
    // `var`: the closure from pass 0 reads the SHARED binding, which the
    // unfolded pass 1 has already pointed at a fresh object. `let`: each pass
    // has its own binding, so pass 0's closure reads pass 0's object either
    // way.
    let stdout = compile_and_run(
        r#"
function viaVar(): string {
  const fns: Array<() => any> = [];
  const seen: string[] = [];
  for (let k = 0; k < 2; k++) {
    const w = { valueOf(): any { if (fns.length) seen.push(fns[0]().v === undefined ? "fresh" : "old"); return k; } };
    var o: any = {};
    o.v = -(w as any);
    fns.push(() => o);
  }
  return seen.join(",");
}
function viaLet(): string {
  const fns: Array<() => any> = [];
  const seen: string[] = [];
  for (let k = 0; k < 2; k++) {
    const w = { valueOf(): any { if (fns.length) seen.push(fns[0]().v === undefined ? "fresh" : "old"); return k; } };
    let o: any = {};
    o.v = -(w as any);
    fns.push(() => o);
  }
  return seen.join(",");
}
try { console.log("var=" + viaVar()); } catch (e: any) { console.log("var threw " + e.name); }
try { console.log("let=" + viaLet()); } catch (e: any) { console.log("let threw " + e.name); }
"#,
    );
    assert_eq!(stdout, "var=fresh\nlet=old\n");
}

#[test]
fn a_closure_created_after_the_builder_still_reads_the_built_object() {
    let stdout = compile_and_run(
        r#"
function build(r: number, i: number): any {
  const o: any = {};
  o.a = i; o.b = r + i; o.c = -r;
  const peek = () => o;
  return peek();
}
console.log(JSON.stringify(build(2, 5)));
"#,
    );
    assert_eq!(stdout, "{\"a\":5,\"b\":7,\"c\":-2}\n");
}

#[test]
fn a_global_getter_that_reads_the_builder_sees_the_allocated_object() {
    let stdout = compile_and_run(
        r#"
// @ts-nocheck
function run() {
  Object.defineProperty(globalThis, "gObs", { get() { return typeof o; }, configurable: true });
  const o = {};
  o.a = gObs;
  return String(o.a);
}
try { console.log(run()); } catch (e) { console.log("threw " + e.name); }
"#,
    );
    assert_eq!(stdout, "object\n");
}
