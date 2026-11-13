use std::io::Write;
use std::process::{Command, Stdio};

fn run_web_source(source: &str) -> String {
    run_web_source_then(source, "")
}

/// `run_web_source`, then `after_boot` (plain JS) in the runtime's scope, for an
/// assertion about a runtime helper that TypeScript cannot reach.
fn run_web_source_then(source: &str, after_boot: &str) -> String {
    let ast = perry_parser::parse_typescript(source, "splice.ts").unwrap();
    let module = perry_hir::lower::lower_module(&ast, "splice", "splice.ts").unwrap();
    let html =
        crate::compile_modules_to_wasm_html(&[("splice".into(), module)], "splice", false).unwrap();
    let runtime = html
        .split("<script>")
        .nth(1)
        .unwrap()
        .split("</script>")
        .next()
        .unwrap();
    let b64 = html
        .split("window.__perryWasmB64 = \"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let script = format!(
        r#"
globalThis.window = globalThis;
const stub = {{appendChild() {{}}, style: {{}}}};
globalThis.document = {{createElement: () => ({{...stub}}), getElementById: () => null, head: stub, body: stub}};
(async () => {{
{runtime}
await bootPerryWasm("{b64}");
{after_boot}
}})().catch(e => {{ console.error(e); process.exitCode = 1; }});
"#
    );
    let mut child = Command::new("node")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Node is required for WASM semantics tests");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    // Node exits 0 while writing warnings to stderr (inherited NODE_OPTIONS,
    // experimental flags), so only the exit status gates the run here.
    String::from_utf8(result.stdout).unwrap().trim().to_string()
}

#[test]
fn splice_inserts_and_evaluates_item_side_effects() {
    assert_eq!(
        run_web_source("const a = [1, 2]; let n = 0; a.splice(1, 0, ++n); console.log(a.join(',')); console.log(n);"),
        "1,1,2\n1"
    );
}

#[test]
fn splice_evaluates_arguments_left_to_right_before_mutating() {
    assert_eq!(
        run_web_source(
            r#"
const a = [10, 20, 30];
let order = 0;
function step(value: number): number { order = order * 10 + value; return value; }
const removed = a.splice(step(1), step(2), step(3), a.length, step(4));
console.log(order);
console.log(a.join(','));
console.log(removed.join(','));
"#
        ),
        "1234\n10,3,3,4\n20,30"
    );
}

#[test]
fn splice_preserves_deletion_and_zero_item_behavior() {
    assert_eq!(
        run_web_source(
            r#"
const a = [1, 2, 3, 4];
console.log(a.splice(1, 2).join(','));
console.log(a.join(','));
console.log(a.splice(0, 0).length);
console.log(a.join(','));
console.log(a.splice(-1).join(','));
console.log(a.join(','));
"#
        ),
        "2,3\n1,4\n0\n1,4\n4\n1"
    );
}

#[test]
fn splice_coerces_non_numeric_start_like_node() {
    assert_eq!(
        run_web_source(
            r#"
const a = [1, 2, 3];
a.splice("1" as any, 0, 9);
console.log(a.join(','));
"#
        ),
        "1,9,2,3"
    );
}

#[test]
fn global_is_nan_and_is_finite_coerce_like_node() {
    assert_eq!(
        run_web_source(
            r#"
const s: any = "abc";
const n: any = "1";
const u: any = undefined;
console.log(isNaN(s) + " " + isNaN(n) + " " + isNaN(u) + " " + isNaN(NaN) + " " + isNaN(1));
console.log(isFinite(n) + " " + isFinite(s) + " " + isFinite(Infinity) + " " + isFinite(1));
"#
        ),
        "true false true true false\ntrue false false true"
    );
}

#[test]
fn console_log_prints_every_argument_on_one_line() {
    assert_eq!(
        run_web_source(
            r#"
const n: any = 1;
console.log("a", n, true);
console.log();
console.log("b");
"#
        ),
        "a 1 true\n\nb"
    );
}

// An async body is emitted as JS, where a class name is not a constructor (#14589).
#[test]
fn new_inside_an_async_body_builds_literals_and_user_classes() {
    assert_eq!(
        run_web_source(
            r#"
class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
  sum(): number { return this.x + this.y; }
}
class Bare { tag(): string { return "bare"; } }
async function f(): Promise<string> {
  const o = { a: 1, b: 2 };
  const p = new Point(3, 4);
  const b = new Bare();
  return (o.a + o.b) + " " + p.sum() + " " + b.tag();
}
f().then((v) => console.log(v)).catch((e) => console.log("threw " + e));
"#
        ),
        "3 7 bare"
    );
}

// A user class's method wins over what its `{__class__}` instance inherits from
// Object.prototype, also through a subclass.
#[test]
fn async_body_method_call_reaches_a_user_override_of_an_object_prototype_name() {
    assert_eq!(
        run_web_source(
            r#"
class T { toString(): string { return "T!"; } }
class U extends T {}
class V extends T { other(): string { return "o"; } }
async function g(): Promise<string> {
  const t = new T();
  const u = new U();
  const v = new V();
  return t.toString() + " " + u.toString() + " " + v.toString() + v.other();
}
g().then((v) => console.log(v)).catch((e) => console.log("threw " + e));
"#
        ),
        "T! T! T!o"
    );
}

// The method property is read once, so an inherited getter runs once.
#[test]
fn async_method_lookup_reads_the_property_once() {
    assert_eq!(
        run_web_source_then(
            "",
            r#"
let reads = 0;
class Base {}
Object.defineProperty(Base.prototype, "m", { get() { reads++; return function () { return "ok"; }; } });
const o = Object.create(Base.prototype);
o.__class__ = "NoSuchClass";
console.log(__jsMethodInvoke(__jsMethodGet(o, "m"), []) + " " + reads);
"#
        ),
        "ok 1"
    );
}

// A call to a method nobody defines throws TypeError, as in JS, instead of returning
// undefined; and a constructor that throws rejects the async function.
#[test]
fn async_body_absent_method_and_throwing_constructor_fail_like_node() {
    assert_eq!(
        run_web_source(
            r#"
class K { a(): number { return 1; } }
async function absent(): Promise<string> {
  const k: any = new K();
  return k.missing();
}
absent().then((v) => console.log("absent: " + v)).catch((e) => console.log("absent threw " + e.name));
class Boom { constructor() { throw new Error("boom"); } }
async function build(): Promise<string> {
  const b = new Boom();
  return "constructed";
}
try {
  build().then((v) => console.log("build: " + v)).catch((e) => console.log("build rejected " + e.message));
} catch (e) {
  console.log("sync caught");
}
"#
        ),
        "absent threw TypeError\nbuild rejected boom"
    );
}

// `r.run(shadow(r))` looks `run` up before it evaluates the argument: the argument gives
// the instance an own, non-function `run`, which a lookup made afterwards would find.
#[test]
fn async_body_method_call_looks_up_the_method_before_the_arguments() {
    assert_eq!(
        run_web_source(
            r#"
class R { run(x: string): string { return "proto " + x; } }
function shadow(r: any): string { r.run = 5; return "arg"; }
async function f(): Promise<string> {
  const r: any = new R();
  return r.run(shadow(r));
}
f().then((v) => console.log(v)).catch((e) => console.log("threw " + e));
"#
        ),
        "proto arg"
    );
}
