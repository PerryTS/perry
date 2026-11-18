use std::io::Write;
use std::process::{Command, Stdio};

fn run_web_source(source: &str) -> String {
    run_web_source_then(source, "")
}

/// `run_web_source`, then `after_boot` (plain JS) in the runtime's scope, for an
/// assertion about a runtime helper that TypeScript cannot reach.
fn run_web_source_then(source: &str, after_boot: &str) -> String {
    run_web_source_with_dom(source, STUB_DOM, after_boot)
}

/// The runtime injects a `<style>` at load, so `document` must exist even for
/// programs that never build a widget.
const STUB_DOM: &str = r#"
const stub = {appendChild() {}, style: {}};
globalThis.document = {createElement: () => ({...stub}), getElementById: () => null, head: stub, body: stub};
"#;

/// Enough of the DOM for a perry/ui widget to be built, inspected and sent an
/// event: an element tree with text, attributes as properties, listeners that
/// `dispatch(type)` fires, and descendant tag selectors for `querySelector`.
const WIDGET_DOM: &str = r#"
class FakeElement {
  constructor(tag) { this.tagName = tag.toUpperCase(); this.children = []; this.style = {}; this.listeners = {}; this.text = ""; }
  appendChild(child) { this.children.push(child); return child; }
  addEventListener(type, fn) { (this.listeners[type] ??= []).push(fn); }
  dispatch(type) { for (const fn of this.listeners[type] ?? []) fn({ type, target: this }); }
  get textContent() { return this.text + this.children.map(c => c.textContent).join(""); }
  set textContent(value) { this.text = String(value); this.children = []; }
  set innerHTML(value) { this.text = ""; this.children = []; }
  setAttribute(name, value) { this[name] = String(value); }
  descendants() { return this.children.flatMap(c => [c, ...c.descendants()]); }
  querySelector(selector) {
    let found = this;
    for (const tag of selector.toUpperCase().split(" ")) {
      found = found.descendants().find(e => e.tagName === tag);
      if (!found) return null;
    }
    return found;
  }
}
globalThis.document = {createElement: tag => new FakeElement(tag), getElementById: () => null, head: new FakeElement("head"), body: new FakeElement("body")};
"#;

/// `run_web_source_then` against `WIDGET_DOM`; `after_boot` finds the widgets in
/// the runtime's `uiHandles`.
fn run_web_ui_source_then(source: &str, after_boot: &str) -> String {
    run_web_source_with_dom(source, WIDGET_DOM, after_boot)
}

fn run_web_source_with_dom(source: &str, dom: &str, after_boot: &str) -> String {
    let ast = perry_parser::parse_typescript(source, "splice.ts").unwrap();
    let mut module = perry_hir::lower::lower_module(&ast, "splice", "splice.ts").unwrap();
    // The HIR transforms `perry --target web` runs before wasm emission.
    perry_transform::inline_finally_into_returns(&mut module);
    perry_transform::transform_generators_without_private_frames(&mut module);
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
{dom}
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

// `instanceof` a built-in constructor asks the host for host objects (a caught
// SyntaxError, an Array), and a user-class instance is also an instance of Object.
#[test]
fn instanceof_answers_for_host_objects_and_object() {
    assert_eq!(
        run_web_source(
            r#"
let caught: any = null;
try { JSON.parse("{"); } catch (e) { caught = e; }
console.log(caught.name + " " + (caught instanceof SyntaxError) + " " + (caught instanceof Error) + " " + (caught instanceof TypeError));
const a: any = [1];
console.log((a instanceof Array) + " " + (a instanceof Object) + " " + (a instanceof Error));
class P {}
class Q extends P {}
const q: any = new Q();
console.log((q instanceof Q) + " " + (q instanceof P) + " " + (q instanceof Object) + " " + (q instanceof Array));
"#
        ),
        "SyntaxError true true false\ntrue true false\ntrue true true false"
    );
}

// UTC getters only, so the expected output does not depend on the host time zone.
#[test]
fn date_constructor_keeps_its_argument_and_getters_read_it() {
    assert_eq!(
        run_web_source(
            r#"
const iso = "2026-11-12T21:42:35.405Z";
const fromString = new Date(iso);
const fromNumber = new Date(1794519755405);
console.log(fromString.getTime());
console.log(fromNumber.toISOString());
console.log(fromString.getUTCFullYear() + "-" + fromString.getUTCMonth() + "-" + fromString.getUTCDate() + " " + fromString.getUTCHours() + ":" + fromString.getUTCMinutes());
console.log(Date.parse(iso));
console.log(new Date().getTime() > 0);
const invalid = new Date(undefined as any).getTime();
console.log(invalid !== invalid);
"#
        ),
        "1794519755405\n2026-11-12T21:42:35.405Z\n2026-10-12 21:42\n1794519755405\ntrue\ntrue"
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

/// A console call is `undefined` wherever it sits.
#[test]
fn console_call_is_undefined_in_every_expression_position() {
    assert_eq!(
        run_web_source(
            r#"
const c = Date.now() > 0;
const r = c ? console.log("a") : 2;
console.log(r);
const f = (x: number) => console.log(x);
console.log(f(3));
"#
        ),
        "a\nundefined\n3\nundefined"
    );
}

/// With namespace pairs on, `console.log(2)` lowers as
/// `NamespacePatched("console") ? <generic call> : <console call>`.
#[test]
fn namespace_pair_with_a_console_arm_validates() {
    let mut pairs = perry_hir::PatchedBuiltins::new();
    pairs.namespace_pairs = true;
    perry_hir::set_patched_builtins(std::sync::Arc::new(pairs));
    let source = "console.log(2); console.log(Math.max(1, 3));";
    let ast = perry_parser::parse_typescript(source, "pair.ts").unwrap();
    let module = perry_hir::lower::lower_module(&ast, "pair", "pair.ts").unwrap();
    assert!(
        format!("{:?}", module.init).contains("NamespacePatched(\"console\")"),
        "the console call must lower as the pair: {:?}",
        module.init
    );
    assert_eq!(run_web_source(source), "2\n3");
    perry_hir::clear_patched_builtins();
}

/// An unsupported `console` method still evaluates its arguments.
#[test]
fn unsupported_console_method_evaluates_its_arguments() {
    assert_eq!(
        run_web_source(
            r#"
import * as c from "node:console";
c.time(String(c.log("a")));
c.log("b");
"#
        ),
        "a\nb"
    );
}

#[test]
fn json_stringify_honors_value_replacer_and_space() {
    assert_eq!(
        run_web_source(
            r#"
const o: any = { a: 1, b: "x", c: [true, null] };
console.log(JSON.stringify(o));
console.log(JSON.stringify(o, ["a", "b"]));
console.log(JSON.stringify(o, (k: string, v: any) => (k === "a" ? 2 : v)));
console.log(JSON.stringify({ d: 1 }, null, "--"));
console.log(JSON.stringify(undefined) === undefined);
"#
        ),
        "{\"a\":1,\"b\":\"x\",\"c\":[true,null]}\n{\"a\":1,\"b\":\"x\"}\n{\"a\":2,\"b\":\"x\",\"c\":[true,null]}\n{\n--\"d\": 1\n}\ntrue"
    );
}

#[test]
fn a_closure_under_any_expression_is_emitted() {
    assert_eq!(
        run_web_source(
            r#"
console.log([1, 2].map((x: number) => x * 2).join(","));
class Static { static value = (() => 5)(); }
console.log(Static.value);
"#
        ),
        "2,4\n5"
    );
}

#[test]
fn an_instance_serializes_only_its_own_fields() {
    assert_eq!(
        run_web_source(
            r#"
class Point { x: number; constructor() { this.x = 1; } }
const p = new Point();
console.log(JSON.stringify(p) + " " + Object.keys(p).join(","));
const q: any = { y: 2 };
console.log(JSON.stringify({ ...q }) + " " + Object.keys(q).join(","));
"#
        ),
        "{\"x\":1} x\n{\"y\":2} y"
    );
}

#[test]
fn a_class_without_a_constructor_runs_its_implicit_one() {
    assert_eq!(
        run_web_source(
            r#"
class A { x: number; constructor(v: number) { this.x = v; } }
class B extends A {}
class C extends A { y = 2; }
class D { z = 1; }
class E extends D {}
const c = new C(4);
console.log(new B(3).x + " " + c.x + " " + c.y + " " + new D().z + " " + new E().z);
"#
        ),
        "3 4 2 1 1"
    );
}

#[test]
fn a_derived_class_initializes_its_fields_after_super() {
    assert_eq!(
        run_web_source(
            r#"
class A { seen: any; constructor() { this.seen = (this as any).y; } }
class B extends A { y = 2; }
class C extends A { y = 3; constructor() { super(); } }
const b = new B();
const c = new C();
console.log(b.seen + " " + (b as any).y + " " + c.seen + " " + (c as any).y);
"#
        ),
        "undefined 2 undefined 3"
    );
}

// `new Map()` without type arguments is a `NewDynamic` of the global constructor;
// a host constructor has to be constructed, not called.
#[test]
fn new_of_a_host_constructor_constructs_it() {
    assert_eq!(
        run_web_source(
            r#"
const m = new Map();
m.set("a", 1);
m.set("b", 2);
console.log(m.size + " " + m.get("b") + " " + m.has("a"));
const s = new Set([1, 2, 2, 3]);
console.log(s.size + " " + s.has(2));
function inner(): number { const w = new Map(); w.set(1, 1); return w.size; }
console.log(inner());
"#
        ),
        "2 2 true\n3 true\n1"
    );
}

// `==` / `!=` (and the `?.` guard, which is `base == null`) follow JS loose equality.
#[test]
fn loose_equality_follows_js() {
    assert_eq!(
        run_web_source(
            r#"
const n: any = null;
const u: any = undefined;
const z: any = 0;
const e: any = "";
const s1: any = "1";
const o: any = { value: 7 };
const p: any = { value: 7 };
console.log((n == null) + " " + (u == null) + " " + (z == null) + " " + (e == z) + " " + (s1 == 1));
console.log((NaN == NaN) + " " + (o == o) + " " + (o == p) + " " + (z != 0) + " " + (s1 != "2"));
function take(): any { return null; }
console.log(String(take()?.value) + " " + String(o?.value));
"#
        ),
        "true true false true true\nfalse true false false true\nundefined 7"
    );
}

// A compiled method read as a value is a function, the same one each time (also
// through a subclass), so the `o.m?.()` guard lets the call through.
#[test]
fn method_read_as_a_value_is_one_function() {
    assert_eq!(
        run_web_source(
            r#"
class R { x = 3; constructor() {} read(n: number): number { return this.x + n; } }
class S extends R { constructor() { super(); } }
const r: any = new R();
const s: any = new S();
console.log(typeof r.read + " " + (r.read === r.read) + " " + (r.read === s.read) + " " + (r.read == null));
console.log(r.read?.(1) + " " + s.read?.(2) + " " + String(r.missing?.(1)));
"#
        ),
        "function true true false\n4 5 undefined"
    );
}

// A method value runs the implementation of the class it was read from, with the
// receiver it is called on, and works as an array callback.
#[test]
fn method_value_keeps_its_implementation_and_serves_as_a_callback() {
    assert_eq!(
        run_web_source(
            r#"
class C { k = 40; constructor() {} answer(): number { return 42; } plus(n: number): number { return n + 1; } }
const c: any = new C();
console.log([1, 2].map(c.plus).join(",") + " " + [1, 2].filter(c.answer).length);
class R { x = 3; constructor() {} read(n: number): number { return this.x + n; } }
class S extends R { constructor() { super(); } read(n: number): number { return 100 + n; } }
const r: any = new R();
const s: any = new S();
const plain: any = { x: 10 };
const f = r.read;
console.log(f.call(s, 1) + " " + f.call(plain, 1) + " " + s.read(1));
"#
        ),
        "2,3 2\n4 11 101"
    );
}

// A method call or a property read on null / undefined throws TypeError.
#[test]
fn null_and_undefined_receivers_throw_type_error() {
    assert_eq!(
        run_web_source(
            r#"
const n: any = null;
const u: any = undefined;
try { n.foo(); console.log("null call: no throw"); } catch (e) { console.log("null call: " + e.name); }
try { u.foo(); console.log("undefined call: no throw"); } catch (e) { console.log("undefined call: " + e.name); }
try { const v = n.bar; console.log("null read: no throw"); } catch (e) { console.log("null read: " + e.message); }
"#
        ),
        "null call: TypeError\nundefined call: TypeError\nnull read: Cannot read properties of null (reading 'bar')"
    );
}

// Computed reads (`n[key]`, `u[0]`) on null / undefined throw too, while `?.[...]`
// still yields undefined.
#[test]
fn computed_reads_on_nullish_receivers_throw() {
    assert_eq!(
        run_web_source(
            r#"
const n: any = null;
const u: any = undefined;
const k: any = "bar";
const xs: number[] | null = null as any;
try { const v = n[k]; console.log("null[key]: no throw"); } catch (e) { console.log("null[key]: " + e.message); }
try { const v = u[0]; console.log("undefined[0]: no throw"); } catch (e) { console.log("undefined[0]: " + e.name); }
try { const v = (xs as any)[1]; console.log("null array[1]: no throw"); } catch (e) { console.log("null array[1]: " + e.name); }
console.log(String(n?.[k]) + " " + String(u?.[0]));
"#
        ),
        "null[key]: Cannot read properties of null (reading 'bar')\nundefined[0]: TypeError\nnull array[1]: TypeError\nundefined undefined"
    );
}

// The Number predicates do not coerce: a string is never NaN, finite or an integer.
#[test]
fn number_predicates_answer_like_node() {
    assert_eq!(
        run_web_source(
            r#"
const s: any = "1";
console.log(Number.isNaN(NaN) + " " + Number.isNaN(s) + " " + Number.isNaN(1));
console.log(Number.isFinite(1) + " " + Number.isFinite(s) + " " + Number.isFinite(Infinity));
console.log(Number.isInteger(5) + " " + Number.isInteger(5.5) + " " + Number.isInteger(s));
console.log(Number.isSafeInteger(9007199254740991) + " " + Number.isSafeInteger(9007199254740992) + " " + Number.isSafeInteger(s));
"#
        ),
        "true false false\ntrue false false\ntrue false false\ntrue false false"
    );
}

#[test]
fn uri_globals_encode_decode_and_throw_like_node() {
    assert_eq!(
        run_web_source(
            r#"
console.log(encodeURIComponent("補 網&x=1"));
console.log(decodeURIComponent("%E8%A3%9C%20%E7%B6%B2"));
console.log(encodeURI("/a b?q=補"));
console.log(decodeURI("/a%20b"));
try { decodeURIComponent("%E0%A4%A"); console.log("no throw"); } catch (e) { console.log("threw"); }
"#
        ),
        "%E8%A3%9C%20%E7%B6%B2%26x%3D1\n補 網\n/a%20b?q=%E8%A3%9C\n/a b\nthrew"
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

// A user class's method wins over what its instance inherits from Object.prototype,
// also through a subclass.
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
Object.defineProperty(o, __classTag, { value: "NoSuchClass" });
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

// A compiled method that throws inside an async body rejects the async function, even
// when a synchronous `try` encloses the call that starts it.
#[test]
fn async_body_user_method_exception_rejects_the_async_function() {
    assert_eq!(
        run_web_source(
            r#"
class Bad { boom(): string { throw new Error("boom"); } }
async function run(): Promise<string> {
  const b = new Bad();
  b.boom();
  return "continued";
}
try {
  run().then((v) => console.log("run: " + v)).catch((e) => console.log("run rejected " + e.message));
} catch (e) {
  console.log("sync caught");
}
"#
        ),
        "run rejected boom"
    );
}

// An async body is emitted as JavaScript, and every `JSON.stringify` reaches it as
// `JsonStringifyFull`: a request body built there must be the JSON text, and the
// replacer and space arguments must apply.
#[test]
fn async_body_json_stringify_feeds_a_request_body() {
    assert_eq!(
        run_web_source(
            r#"
function tok(): string { return "Bearer " + String(6 * 7); }
async function f(obj: any): Promise<string> {
  const req = new Request("http://127.0.0.1/x", { method: "POST", body: JSON.stringify(obj) });
  const pretty = JSON.stringify([1], null, 2);
  const keys = JSON.stringify(obj, ["a", "t"]);
  return (await req.text()) + " " + pretty.length + " " + keys;
}
f({ a: 1, b: [2, 3], t: tok() }).then((v) => console.log(v)).catch((e) => console.log("threw " + e));
"#
        ),
        r#"{"a":1,"b":[2,3],"t":"Bearer 42"} 7 {"a":1,"t":"Bearer 42"}"#
    );
}

// `__proto__` is inherited by the bridge-name map, not an entry of it: calling it on a
// user instance throws TypeError instead of reaching the bridge fallback.
#[test]
fn async_body_call_of_an_inherited_map_key_throws_type_error() {
    assert_eq!(
        run_web_source(
            r#"
class K { a(): number { return 1; } }
async function proto(): Promise<string> {
  const k: any = new K();
  return k.__proto__();
}
proto().then((v) => console.log("proto: " + v)).catch((e) => console.log("proto threw " + e.name));
"#
        ),
        "proto threw TypeError"
    );
}

// A bare read of a host getter (`res.status`) reaches an async body as a native call
// with no arguments, as `res.text()` does: the getter yields its value and the method
// is still called.
#[test]
fn async_body_reads_host_getters_and_calls_host_methods() {
    assert_eq!(
        run_web_source(
            r#"
async function f(): Promise<string> {
  const r = await fetch("data:text/plain,hi");
  const head = r.status + " " + r.ok + " " + r.statusText + " " + r.headers.get("content-type");
  return head + " " + (await r.text());
}
f().then((v) => console.log(v)).catch((e) => console.log("threw " + e));
"#
        ),
        "200 true OK text/plain hi"
    );
}

// A zero-argument native method the object lacks still throws, as the call would.
#[test]
fn async_body_call_of_a_missing_host_method_throws() {
    assert_eq!(
        run_web_source(
            r#"
async function f(): Promise<string> {
  const r = await fetch("data:text/plain,hi");
  return r.missing();
}
f().then((v) => console.log("value " + v)).catch((e) => console.log("threw " + e));
"#
        ),
        "threw TypeError: missing is not a function"
    );
}

// A method name is any string (`q["it's"]()`); the async JS emitter quotes it as a literal.
#[test]
fn an_async_body_calls_a_method_whose_name_needs_escaping() {
    assert_eq!(
        run_web_source(
            r#"
class Q { "it's"(): string { return "quoted"; } }
async function f(): Promise<string> { const q: any = new Q(); return q["it's"](); }
f().then((v) => console.log(v)).catch((e) => console.log("threw " + e));
"#
        ),
        "quoted"
    );
}

// `fetchWithAuth` / `fetchPostWithAuth` in an async body send the request the WASM arms
// send: the auth header as `Authorization`, POST with the body. A stub `fetch` echoes
// what it received; the timer lets it be installed after boot.
#[test]
fn async_body_fetch_with_auth_sends_the_auth_header() {
    assert_eq!(
        run_web_source_then(
            r#"
async function f(): Promise<string> {
  const g = await fetchWithAuth("http://h/g", "Bearer a");
  const p = await fetchPostWithAuth("http://h/p", "Bearer b", '{"x":1}');
  return (await g.text()) + " " + (await p.text());
}
setTimeout(() => { f().then((v) => console.log(v)).catch((e) => console.log("threw " + e)); }, 0);
"#,
            r#"globalThis.fetch = async (url, init) => new Response(
  [url, init.method ?? "GET", init.body ?? "-", init.headers.Authorization].join("|"));"#
        ),
        r#"http://h/g|GET|-|Bearer a http://h/p|POST|{"x":1}|Bearer b"#
    );
}

// A property name is any string; an async body reads, writes and builds it as a literal.
#[test]
fn an_async_body_handles_property_names_that_need_escaping() {
    assert_eq!(
        run_web_source(
            r#"
async function f(): Promise<string> {
  const o: any = { "it's": 1, "a\"b": 2 };
  o["it's"] = o["it's"] + 10;
  return o["it's"] + " " + o["a\"b"];
}
f().then((v) => console.log(v)).catch((e) => console.log("threw " + e));
"#
        ),
        "11 2"
    );
}

#[test]
fn table_update_row_count_renders_the_new_rows() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { Table, Text, tableSetColumnHeader, tableUpdateRowCount } from "perry/ui";
const table = Table(3, 1, (row: number, col: number) => Text("r" + row));
tableSetColumnHeader(table, 0, "Name");
tableUpdateRowCount(table, 4);
"#,
            r#"
const table = [...uiHandles.values()].find(e => e.tagName === "TABLE");
const rows = table.querySelector("tbody").children;
console.log(table.querySelector("thead tr").textContent + " " + rows.length + " " + rows[rows.length - 1].textContent);
"#
        ),
        "Name 4 r3"
    );
}

#[test]
fn closures_share_the_variables_they_mutate() {
    assert_eq!(
        run_web_source(
            r#"
function counter() {
  let n = 0;
  const add = (x: number) => { n = n + x; return n; };
  const read = () => n;
  return [add, read];
}
const c: any = counter();
console.log(c[0](1) + " " + c[0](2) + " " + c[1]());
function machine() {
  let state = 0;
  return () => { switch (state) { case 0: state = 1; return "a"; case 1: state = 2; return "b"; default: return "-"; } };
}
const m = machine();
console.log(m() + m() + m());
let total = 0;
function bump(start: number) { const inc = () => { start++; total++; }; inc(); inc(); return start; }
console.log(bump(5) + " " + total);
"#
        ),
        "1 3 3\nab-\n7 2"
    );
}

// `Image(url, alt?)` and `Image({url, alt?})` keep the alt text; `Image({systemName})`
// is a symbol, as `ImageSymbol(name)` is.
#[test]
fn image_keeps_alt_text_in_both_forms() {
    assert_eq!(
        run_web_source_then(
            r#"
import { Image } from "perry/ui";
Image("x.png", "photo");
Image({ url: "y.png", alt: "avatar" });
Image({ systemName: "gear" });
"#,
            r#"
for (const e of uiHandles.values()) console.log(e.src !== undefined ? e.src + " " + e.alt : e.textContent);
"#
        ),
        "x.png photo\ny.png avatar\n\u{2b1c} gear"
    );
}

#[test]
fn a_generator_resumes_returns_and_throws_like_node() {
    assert_eq!(
        run_web_source(
            r#"
function* g() { yield 1; try { yield 2; } finally { console.log("fin"); } }
const a = g();
console.log(JSON.stringify(a.next()) + JSON.stringify(a.next()) + JSON.stringify(a.next()));
const b = g();
b.next();
b.next();
console.log(JSON.stringify(b.return(7)));
const c = g();
try { c.throw("boom"); } catch (e) { console.log("threw " + e); }
console.log(JSON.stringify(c.next()));
function* echo() { const x = yield "first"; yield "got " + x; }
const d = echo();
d.next();
console.log(d.next("hi").value);
"#
        ),
        "fin\n{\"value\":1,\"done\":false}{\"value\":2,\"done\":false}{\"done\":true}\nfin\n{\"value\":7,\"done\":true}\nthrew boom\n{\"done\":true}\ngot hi"
    );
}

#[test]
fn section_renders_its_children_after_the_legend() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { Section, Text } from "perry/ui";
Section("Contact", [Text("phone"), Text("email")]);
"#,
            r#"
const fieldset = [...uiHandles.values()].find(e => e.tagName === "FIELDSET");
console.log(fieldset.children.map(c => c.tagName + ":" + c.textContent).join(" "));
"#
        ),
        "LEGEND:Contact SPAN:phone SPAN:email"
    );
}

#[test]
fn for_of_and_destructuring_drive_the_iterator_protocol() {
    assert_eq!(
        run_web_source(
            r#"
function* g() { yield 1; try { yield 2; yield 3; } finally { console.log("closed"); } }
let seen = "";
for (const x of g()) { seen += x; if (x === 2) break; }
console.log(seen);
const m: any = new Map([["a", 1], ["b", 2]]);
for (const [k, v] of m) console.log(k + "=" + v);
let chars = "";
const s: any = "hé";
for (const ch of s) chars += "[" + ch + "]";
console.log(chars);
const src: any = new Set([7, 8, 9]);
const [first, ...rest] = src;
console.log(first);
console.log(rest.join(","));
function pair(): any { return [4, 5]; }
const [p, q] = pair();
console.log(p + q);
"#
        ),
        "closed\n12\na=1\nb=2\n[h][é]\n7\n8,9\n9"
    );
}

// perry/ui declares `Picker(onChange)`, so the closure arrives as the first argument.
#[test]
fn picker_on_change_is_called_with_the_chosen_index() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { Picker, pickerAddItem } from "perry/ui";
const picker = Picker((index: number) => { console.log("picker " + index); });
pickerAddItem(picker, "first");
pickerAddItem(picker, "second");
"#,
            r#"
const select = [...uiHandles.values()].find(e => e.tagName === "SELECT");
select.selectedIndex = 1;
select.dispatch("change");
"#
        ),
        "picker 1"
    );
}

#[test]
fn a_generator_keeps_its_locals_across_resumes() {
    assert_eq!(
        run_web_source(
            r#"
function* naturals() { let n = 0; while (true) { yield n++; } }
const first: number[] = [];
for (const n of naturals()) { if (n >= 4) break; first.push(n); }
console.log(first.join(","));
"#
        ),
        "0,1,2,3"
    );
}

// `super.m()` runs the parent's `m` on the same receiver, even when the receiver's
// class overrides `m`, and skips a parent that does not define it.
#[test]
fn a_super_method_call_runs_the_parent_implementation() {
    assert_eq!(
        run_web_source(
            r#"
class A { k = "a"; who(n: number): string { return "A" + n + this.k; } }
class B extends A { who(n: number): string { return "B>" + super.who(n + 1); } }
class C extends B { k = "c"; }
class D extends C { who(n: number): string { return "D>" + super.who(n * 10); } }
console.log(new B().who(1));
console.log(new D().who(2));
class E extends A { r: string; constructor() { super(); this.r = super.who(5); } who(n: number): string { return "E"; } }
console.log(new E().r);
"#
        ),
        "B>A2a\nD>B>A21c\nA5a"
    );
}

// An accessor on a class in the walk shadows an ancestor's method of that name:
// `super.m()` calls what B's `get m` returns, not A's `m`.
#[test]
fn a_super_method_call_reads_a_shadowing_getter() {
    assert_eq!(
        run_web_source(
            r#"
class A { m(): string { return "A.m"; } }
class B extends A { get m(): any { return () => "B.get"; } }
class C extends B { m(): string { return "C>" + super.m(); } }
console.log(new C().m());
"#
        ),
        "C>B.get"
    );
}

// An arrow inside a class method resolves `super` from the method's class.
#[test]
fn a_super_method_call_in_an_arrow_starts_at_the_enclosing_class() {
    assert_eq!(
        run_web_source(
            r#"
class A2 { m(): string { return "A2"; } }
class B2 extends A2 { m(): string { const f = () => super.m(); return "B2>" + f(); } }
console.log(new B2().m());
"#
        ),
        "B2>A2"
    );
}

// A setter-only accessor on a class in the walk ends the lookup: its read value is
// undefined, so `super.m()` throws a TypeError instead of reaching an ancestor's `m`.
#[test]
fn a_super_method_call_stops_at_a_setter_only_accessor() {
    assert_eq!(
        run_web_source(
            r#"
class A { m(): string { return "A.m"; } }
class B extends A { set m(v: any) { } }
class C extends B { m(): string { try { return "C>" + super.m(); } catch (e) { return (e instanceof TypeError) + ""; } } }
console.log(new C().m());
"#
        ),
        "true"
    );
}

// `super.m(args)` reads `super.m` before it evaluates the arguments, as `o.m(args)` does.
#[test]
fn a_super_method_call_reads_the_property_before_the_arguments() {
    assert_eq!(
        run_web_source(
            r#"
let state = 0;
class A { get m(): any { const seen = state; return (x: number) => seen + ":" + x; } }
class B extends A { m(): any { return super.m(state = 1); } }
console.log(new B().m());
"#
        ),
        "0:1"
    );
}

// A getter that returns a non-callable makes `super.m()` throw a TypeError.
#[test]
fn a_super_method_call_of_a_non_callable_getter_result_throws_type_error() {
    assert_eq!(
        run_web_source(
            r#"
class A { get n(): any { return null; } get k(): any { return 42; } }
class B extends A {
  t(f: () => any): string { try { f(); return "no throw"; } catch (e) { return (e instanceof TypeError) + ""; } }
  run(): string { return this.t(() => super.n()) + " " + this.t(() => super.k()); }
}
console.log(new B().run());
"#
        ),
        "true true"
    );
}

// An assignment-pattern default applies to `undefined` (a missing element, a
// missing property) and never to `null` or a falsy value.
#[test]
fn assignment_pattern_defaults_apply_to_undefined() {
    assert_eq!(
        run_web_source(
            r#"
let x: any, y: any, p: any;
[x = 5] = [];
console.log(x);
[x = 5, y = 6] = [undefined, null];
console.log(x, y);
[x = 5] = [0];
console.log(x);
({ a: p = 8 } = {} as any);
console.log(p);
[[x = 1], { b: y = 2 }] = [[], {}] as any;
console.log(x, y);
for ([x = 10] of [[], [7]] as any[]) console.log(x);
"#
        ),
        "5\n5 null\n0\n8\n1 2\n10\n7"
    );
}

/// #14734: a bridge call stores its arguments only into the frame it opened.
/// A slot above the open frame is where the next nested frame is written, so
/// the calls overwrote each other's arguments (`join` on an `any` receiver, and
/// the `class_call_method` fallback that a value-position `o.list.push(v)` takes).
#[test]
fn bridge_calls_store_only_into_the_frame_they_open() {
    assert_eq!(
        run_web_source(
            r#"
function f(s: any) { return s.join(","); }
function g(s: any, t: any) { return s.join("-") + "|" + t.join("+"); }
console.log(f([1, 2]));
console.log("pair", g([1, 2], [3, 4]), g(["a"], ["b", "c"]));
const o = { list: [] as number[] };
console.log("push", o.list.push(7), o.list.push(8), o.list.join(","));
"#
        ),
        "1,2\npair 1-2|3+4 a|b+c\npush 1 2 7,8"
    );
}

// A tagged template calls its tag with the call site's strings array, whose `raw`
// holds the unescaped text and which is the same object on every evaluation, through
// a static method, an instance method or a function.
#[test]
fn a_tagged_template_passes_its_strings_array_to_any_tag() {
    assert_eq!(
        run_web_source(
            r#"
class T { static tag(s: string[], a: any, b: any) { return s.join("|") + "/" + a + "," + b; } }
console.log(T.tag`a${1}b${2}c`);
class K { m = "M"; tag(s: any, v: any) { return this.m + s[0] + v + s[1]; } }
console.log(new K().tag`v=${3}!`);
function raw(s: TemplateStringsArray) { return s.raw[0] + "|" + s.raw.length + "|" + s[0]; }
console.log(raw`a\nb`);
function anyRaw(s: any) { return s.raw[0] + "," + s.raw[1]; }
console.log(anyRaw`x\ty${0}z`);
function id(s: any) { return s; }
function site() { return id`q`; }
const a = site();
console.log(a === site(), a === id`q`, Array.isArray(a), a.length);
"#
        ),
        "a|b|c/1,2\nMv=3!\na\\nb|1|a\nb\nx\\ty,z\ntrue false true 1"
    );
}

/// #14733: every object pattern opens with RequireObjectCoercible on its
/// source, which hands the source through or throws a TypeError for a nullish one.
#[test]
fn object_patterns_bind_from_their_source_and_reject_nullish() {
    assert_eq!(
        run_web_source(
            r#"
const arr = [{ k: 1 }, { k: 2 }];
for (const { k } of arr) console.log("k", k);
const v: any = { a: 3, b: { c: 4 } };
const { a, b: { c } } = v;
console.log("decl", a, c);
let reads = 0;
function src(): any { reads++; return { x: 5 }; }
const { x } = src();
console.log("once", x, reads);
function nullish(o: any): string {
  try { const { y } = o; return "no throw " + y; } catch (e) { return "caught " + (e instanceof TypeError); }
}
console.log(nullish(null), nullish(undefined), nullish({ y: 6 }));
"#
        ),
        "k 1\nk 2\ndecl 3 4\nonce 5 1\ncaught true caught true no throw 6"
    );
}

// `.message` is an ordinary property read: only an Error's message comes from the
// Error itself.
#[test]
fn message_reads_the_property_of_any_object() {
    assert_eq!(
        run_web_source(
            r#"
const o: any = JSON.parse('{"message":"hi","phone":"p"}');
const lit: any = { message: "lit" };
class G { get message(): string { return "getter"; } }
class F { message: string; constructor() { this.message = "field"; } }
const none: any = {};
console.log(o.message + " " + o.phone + " " + lit.message + " " + new G().message + " " + new F().message + " " + none.message);
try { throw { message: "thrown" }; } catch (e: any) { console.log(e.message); }
const err = new Error("boom");
const empty = new Error();
console.log(err.message + " [" + empty.message + "]");
try { JSON.parse("{bad"); } catch (e: any) { console.log(e.message.length > 0); }
"#
        ),
        "hi p lit getter field undefined\nthrown\nboom []\ntrue"
    );
}

#[test]
fn an_error_subclass_takes_its_message_from_super() {
    assert_eq!(
        run_web_source(
            r#"
class MyErr extends Error { constructor(m?: string) { super(m); this.name = "MyErr"; } }
class Coded extends Error { code: number; constructor(m: string, c: number) { super(m); this.code = c; } }
class Bare extends Error {}
class Deep extends Bare { constructor() { super("deep"); } }
class Ranged extends RangeError { constructor(m: string) { super(m); } }
class Fielded extends Error { code = 5; }
const c = new Coded("coded", 7);
console.log(new MyErr("sub").message + " [" + new MyErr().message + "] " + c.message + " " + c.code);
console.log(new Bare("bare").message + " " + new Deep().message + " " + new Ranged("ranged").message + " [" + new Fielded().message + "]");
try { throw new MyErr("caught"); } catch (e: any) { console.log(e.message + " " + (e instanceof Error)); }
console.log(JSON.stringify(new MyErr("hidden")) + " " + Object.keys(new Bare("k")).length);
"#
        ),
        "sub [] coded 7\nbare deep ranged []\ncaught true\n{\"name\":\"MyErr\"} 0"
    );
}

// `textSetFontWeight(widget, size, weight)` sets both; the weight is the native
// 0..1 scale, mapped onto CSS 100..900 and clamped.
#[test]
fn text_set_font_weight_applies_the_size_and_the_weight() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { Text, textSetFontWeight } from "perry/ui";
const a = Text("A");
textSetFontWeight(a, 28, 1.0);
const b = Text("B");
textSetFontWeight(b, 16, 0.5);
const c = Text("C");
textSetFontWeight(c, 12, 700);
"#,
            r#"
for (const label of ["A", "B", "C"]) {
  const el = [...uiHandles.values()].find(e => e.textContent === label);
  console.log(label + " " + el.style.fontSize + " " + el.style.fontWeight);
}
"#
        ),
        "A 28px 900\nB 16px 500\nC 12px 900"
    );
}

// A class used as a value is a constructor: `new` on it runs the class's constructor
// and field initializers, and the instance keeps its methods and its class.
#[test]
fn a_class_used_as_a_value_constructs_its_instances() {
    assert_eq!(
        run_web_source(
            r#"
class K { v = 3; constructor(x?: number) { if (x !== undefined) this.v = x; } twice() { return this.v * 2; } }
class Bare { w = 4; }
const KK: any = K;
const kk = new KK(7);
console.log(typeof KK + " " + kk.v + " " + kk.twice() + " " + (kk instanceof K) + " " + new KK().v);
const r = { K, Bare };
console.log(typeof r.K + " " + new r.K(9).twice() + " " + new r.Bare().w + " " + (KK === r.K));
"#
        ),
        "function 7 14 true 3\nfunction 18 4 true"
    );
}

// `new` on a class value hands its arguments to the compiled constructor as Perry
// values: a closure argument stays a closure the array bridges can call.
#[test]
fn a_class_value_constructor_receives_a_closure_argument() {
    assert_eq!(
        run_web_source(
            r#"
class M { out: number[]; constructor(cb: (n: number) => number) { this.out = [1, 2].map(cb); } }
const MM: any = M;
console.log(new MM((n: number) => n * 10).out.join(","));
"#
        ),
        "10,20"
    );
}

// Host `instanceof` on a class value walks the class table, as compiled `instanceof` does.
#[test]
fn host_instanceof_on_a_class_value_walks_the_class_chain() {
    assert_eq!(
        run_web_source_then(
            r#"
class P { }
class Q extends P { }
const refs: any[] = [P, Q];
console.log(refs.length);
"#,
            r#"
const P = __classRef("P", -1), Q = __classRef("Q", -1);
console.log([__classInstance("Q") instanceof P, __classInstance("Q") instanceof Q,
  __classInstance("P") instanceof Q, ({}) instanceof P, 1 instanceof P].join(" "));
"#
        ),
        "2\ntrue true false false false"
    );
}

// `new` on a class value yields what [[Construct]] yields: the instance, unless the
// constructor returns an object. A bare `return;` keeps the initialized instance.
#[test]
fn a_class_value_constructor_returning_no_object_yields_the_instance() {
    assert_eq!(
        run_web_source(
            r#"
class E { v = 1; constructor(x: number) { if (x > 0) return; this.v = 2; } get() { return this.v; } }
class O { v = 1; constructor() { return { v: 5 } as any; } }
const EE: any = E, OO: any = O;
const e = new EE(1);
console.log(e.v + " " + e.get() + " " + (e instanceof E) + " " + new EE(0).v + " " + new OO().v + " " + (new OO() instanceof O));
"#
        ),
        "1 1 true 2 5 false"
    );
}

// `fetchWithAuth` / `fetchPostWithAuth` build their headers object between
// evaluating their arguments; the bridge must receive each argument in its own
// slot, evaluated in source order, and the module must validate at all.
#[test]
fn fetch_with_auth_passes_url_method_body_and_authorization() {
    assert_eq!(
        run_web_source_with_dom(
            r#"
function base(): string { return "http://h/" + String(6 * 7); }
function tok(): string { return "Bearer " + base().length; }
function arg(label: string, v: string): string { console.log(label); return v; }
fetchWithAuth(arg("url", base() + "/g"), arg("auth", `${tok()}-g`))
  .then(() => fetchPostWithAuth(arg("url", base() + "/p"), arg("auth", tok()), arg("body", JSON.stringify({ n: tok() }))))
  .then(() => console.log("done"));
"#,
            &format!(
                "{STUB_DOM}{}",
                r#"globalThis.fetch = (url, opts) => {
  console.log(JSON.stringify([url, opts]));
  return Promise.resolve({ text: () => Promise.resolve("ok") });
};"#
            ),
            ""
        ),
        concat!(
            "url\nauth\n",
            r#"["http://h/42/g",{"headers":{"Authorization":"Bearer 11-g"}}]"#,
            "\nurl\nauth\nbody\n",
            r#"["http://h/42/p",{"method":"POST","body":"{\"n\":\"Bearer 11\"}","headers":{"Authorization":"Bearer 11"}}]"#,
            "\ndone"
        )
    );
}

#[test]
fn property_update_stores_and_returns_the_old_or_new_value() {
    assert_eq!(
        run_web_source(
            "class P { x: number; constructor(x: number) { this.x = x } }
             const r = new P(5); const a = r.x++; console.log(a, r.x);
             const o: any = { x: 5 };
             const b = o.x++; console.log(b, o.x);
             const d = ++o.x; console.log(d, o.x);
             o.x--; console.log(o.x);
             const e = --o.x; console.log(e, o.x);
             o.x++; console.log(o.x);"
        ),
        "5 6\n5 6\n7 7\n6\n5 5\n6"
    );
}

#[test]
fn member_update_on_this_computed_keys_and_elements() {
    assert_eq!(
        run_web_source(
            "class C {
               n = 0; items: number[] = [0, 0];
               inc(): number { return this.n++; }
               pre(): number { return ++this.n; }
               bump(): number { this.n++; return this.items[1]++; }
             }
             const c = new C();
             console.log(c.inc(), c.n); console.log(c.pre(), c.n);
             console.log(c.bump(), c.n);
             console.log(c.items.join(','));
             const o: any = { y: 10 }; const k = 'y';
             const a = o[k]++; console.log(a, o.y);
             const b = ++o[k]; console.log(b, o.y);"
        ),
        "0 1\n2 2\n0 3\n0,1\n10 11\n12 12"
    );
}

#[test]
fn member_update_evaluates_base_and_key_once() {
    assert_eq!(
        run_web_source(
            "let calls = 0; const o: any = { x: 1 };
             function get(): any { calls++; return o; }
             const a = get().x++; console.log(a, o.x, calls);
             const b = ++get().x; console.log(b, o.x, calls);
             const arr = [5, 6]; let i = 0;
             const c = arr[i++]++; console.log(c, arr.join(','), i);"
        ),
        "1 2 1\n3 3 2\n5 6,6 1"
    );
}

#[test]
fn member_update_converts_the_old_value_to_a_number() {
    assert_eq!(
        run_web_source(
            "const o: any = { x: null, b: true, s: '5' };
             const a = o.x++; console.log(a, o.x);
             const b = o.b++; console.log(b, o.b);
             const c = ++o.s; console.log(c, o.s);
             const arr: any[] = [null]; const d = arr[0]--; console.log(d, arr[0]);"
        ),
        "0 1\n1 2\n6 6\n0 -1"
    );
}

// An async body is emitted as JS, which must lower `Object.keys/values/entries` itself
// rather than read them as undefined (#14633).
#[test]
fn async_body_lists_an_objects_keys_values_and_entries() {
    assert_eq!(
        run_web_source(
            r#"
async function f(): Promise<string> {
  const o: any = { a: 1, b: 2 };
  return Object.keys(o).join("|") + " " + Object.values(o).join("|") + " " + Object.entries(o).join("|");
}
f().then((v) => console.log(v)).catch((e) => console.log("threw " + e));
"#
        ),
        "a|b 1|2 a,1|b,2"
    );
}

// `for...of` over an array and `i++` / `k--` in an async body: the array check of the
// for-of lowering and the update expression both reach the JS emitter (#14595).
#[test]
fn async_body_runs_for_of_over_an_array_and_update_expressions() {
    assert_eq!(
        run_web_source(
            r#"
async function f(xs: string[]): Promise<string> {
  let s = "";
  for (const p of xs) s = s + p;
  let n = 0;
  // The `n < 100` bound ends the loop even where `i++` does nothing.
  for (let i = 0; i < 3 && n < 100; i++) n = n + i + 1;
  let k = 5;
  const before = k--;
  return s + " " + n + " " + before + " " + k + " " + (++k);
}
f(["a", "b"]).then((v) => console.log(v)).catch((e) => console.log("threw " + e));
"#
        ),
        "ab 6 5 4 5"
    );
}

// The `try` lowering declares a flag inside the `try` block and reads it in the `catch`;
// the JS body must declare it where both can see it (#14595).
#[test]
fn async_body_catch_reads_a_local_the_try_block_declared() {
    assert_eq!(
        run_web_source(
            r#"
async function f(): Promise<string> {
  try {
    throw new Error("boom");
  } catch (e) {
    return "caught " + (e as Error).message;
  }
}
f().then((v) => console.log(v)).catch((e) => console.log("threw " + e));
"#
        ),
        "caught boom"
    );
}

// Iterating a non-array and destructuring are not emitted in an async body; they reject
// with a TypeError instead of running on with undefined values.
#[test]
fn async_body_rejects_iteration_it_cannot_emit() {
    let out = run_web_source(
        r#"
async function each(): Promise<string> {
  let s = "";
  for (const v of new Set(["x", "y"])) s = s + v;
  return s;
}
async function pick(xs: number[]): Promise<string> {
  const [a, b] = xs;
  return a + "," + b;
}
each().then((v) => console.log(v)).catch((e) => console.log("threw " + e.name));
pick([1, 2]).then((v) => console.log(v)).catch((e) => console.log("threw " + e.name));
"#,
    );
    assert_eq!(out, "threw TypeError\nthrew TypeError");
}

// A logical operator in an async body evaluates its left operand once, and an `await`
// in its right operand still runs in the async body.
#[test]
fn async_body_logical_operators_evaluate_the_left_operand_once() {
    assert_eq!(
        run_web_source(
            r#"
async function f(): Promise<string> {
  let i = 1;
  const a = i++ || 0;
  let j = 0;
  const b = j++ && 9;
  let k: any = 4;
  const c = k ?? 0;
  const d = (i > 100) || (await Promise.resolve(7));
  return i + "," + a + " " + j + "," + b + " " + c + " " + d;
}
f().then((v) => console.log(v)).catch((e) => console.log("threw " + e));
"#
        ),
        "2,1 1,0 4 7"
    );
}

#[test]
fn a_join_inside_a_call_argument_keeps_the_other_arguments() {
    assert_eq!(
        run_web_source(
            r#"
class Box { items: number[] = [1, 2, 3]; }
const inst = new Box();
const n = 7;
console.log(n, inst.items.join(","));
console.log(inst.items.join("-"), inst instanceof Box);
console.log(n, inst.items.join());
const lit = { out: ["x", "y"] };
console.log(lit.out.join(","), lit instanceof Object);
"#
        ),
        "7 1,2,3\n1-2-3 true\n7 1,2,3\nx,y true"
    );
}

#[test]
fn an_array_bridge_call_opens_its_own_argument_frame() {
    // Only the neighbours are asserted: what these calls return is a separate matter.
    // Writing into the enclosing call's frame replaced them.
    let out = run_web_source(
        r#"
const n = 7;
const a = [3, 1, 2];
console.log(n, a.toReversed(), n);
console.log(n, a.toSorted(), n);
console.log(n, a.toSorted((x, y) => y - x), n);
console.log(n, a.with(0, 9), n);
console.log(n, a.toSpliced(1, 1), n);
console.log(n, a.keys(), n);
console.log(n, a.values(), n);
console.log(n, a.entries(), n);
"#,
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 8, "{out}");
    for line in lines {
        assert!(line.starts_with("7 ") && line.ends_with(" 7"), "{out}");
    }
}

// `Toggle(label, onChange: (value: boolean) => void)` hands the callback a boolean.
#[test]
fn toggle_on_change_receives_a_boolean() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { Toggle } from "perry/ui";
Toggle("Notify", (value: boolean) => { console.log("toggle " + (value === true) + " " + typeof value); });
"#,
            r#"
const toggle = [...uiHandles.values()].find(e => e.tagName === "LABEL");
toggle._inp.checked = true;
toggle._inp.dispatch("change");
"#
        ),
        "toggle true boolean"
    );
}

#[test]
fn a_class_declared_in_a_function_keeps_its_methods() {
    assert_eq!(
        run_web_source(
            r#"
function make(): any {
  class Local extends Error {
    who() { return "local"; }
  }
  return new Local("m");
}
const b = make();
console.log(typeof b.who);
console.log(b.who());
function plain(n: number): any {
  class P { v: number; constructor(v: number) { this.v = v; } twice() { return this.v * 2; } }
  return new P(n);
}
console.log(plain(4).twice());
"#
        ),
        "function\nlocal\n8"
    );
}

// A closure created before a class declaration it reads shares that binding's cell, so
// it sees the class once the declaration has run.
#[test]
fn a_closure_reads_a_class_declared_after_it() {
    assert_eq!(
        run_web_source(
            r#"
function make(tag: string) {
  const read = () => C;
  class C { t() { return tag; } }
  return read() === C;
}
function makeError() {
  const read = () => E;
  class E extends Error {}
  return typeof read() + " " + (read() === E);
}
console.log(make("a") + " " + make("b") + " " + makeError());
"#
        ),
        "true true function true"
    );
}

#[test]
fn an_object_literal_super_call_starts_at_the_literals_prototype() {
    // The methods are read off the object before the call, so the test does not
    // depend on how `o.m()` dispatches a closure property.
    assert_eq!(
        run_web_source(
            r#"
const base = { hi() { return "base"; }, greet(x: string) { return "hello " + x; } };
const o = { __proto__: base, hi() { return "o>" + super.hi(); }, greet(x: string) { return super.greet(x) + "!"; } };
const hi = o.hi;
const greet = o.greet;
console.log(hi());
console.log(greet("w"));
const k = "hi";
const p = { __proto__: o, hi() { return "p>" + super[k](); } };
const phi = p.hi;
console.log(phi());
const q = { __proto__: base, x: 1 };
const qhi = q.hi;
console.log(q.x, qhi());
"#
        ),
        "o>base\nhello w!\np>o>base\n1 base"
    );
}

// The lookup reads the literal's [[Prototype]] when the call runs, and before the
// arguments are evaluated; a null prototype or a non-callable property throws.
#[test]
fn an_object_literal_super_call_resolves_at_call_time() {
    assert_eq!(
        run_web_source(
            r#"
const a = { m() { return "a"; } };
const b = { m() { return "b"; } };
const o = { __proto__: a, m(x: string) { return super.m() + x; }, n() { return super.m(Object.setPrototypeOf(o, b)); } };
const om = o.m;
console.log(om("!"));
Object.setPrototypeOf(o, b);
console.log(om("!"));
Object.setPrototypeOf(o, a);
const on = o.n;
console.log(on());
const z = { __proto__: null, m() { return super.m(); } };
const zm = z.m;
try { zm(); console.log("null: no throw"); } catch (e) { console.log("null: " + e.name); }
const c = { __proto__: { m: 1 }, m() { return super.m(); } };
const cm = c.m;
try { cm(); console.log("1: no throw"); } catch (e) { console.log("1: " + e.name); }
"#
        ),
        "a!\nb!\na\nnull: TypeError\n1: TypeError"
    );
}

#[test]
fn a_class_value_holds_static_side_properties_and_a_prototype() {
    assert_eq!(
        run_web_source(
            r#"
class T {}
(T as any).key = "class";
console.log((T as any).key);
const k = "dyn";
(T as any)[k] = 2;
console.log((T as any)[k]);
const base = { tag: "base" };
class U {}
Object.setPrototypeOf(U, base);
console.log((U as any).tag);
"#
        ),
        "class\n2\nbase"
    );
}

#[test]
fn a_closure_used_as_a_value_is_a_function() {
    assert_eq!(
        run_web_source(
            r#"
const f = () => 1;
function named(): number { return 2; }
console.log(typeof f + " " + typeof (() => 1) + " " + typeof named);
const o: any = {}; o.h = f;
console.log(typeof o.h + " " + o.h() + " " + ({ g(n: number) { return n + 1; } }).g(41));
const fake: any = { funcIdx: 0, captures: [] };
console.log(typeof fake);
"#
        ),
        "function function function\nfunction 1 42\nobject"
    );
}

#[test]
fn an_async_body_calls_a_closure_held_by_an_object() {
    assert_eq!(
        run_web_source(
            r#"
const o: any = {}; o.h = (n: number) => n * 2;
async function main() { console.log(typeof o.h + " " + o.h(21)); }
main();
"#
        ),
        "function 42"
    );
}

#[test]
fn an_async_body_calls_a_closure_held_in_a_local() {
    assert_eq!(
        run_web_source(
            r#"
const o: any = {}; o.h = (n: number) => n * 2;
async function main() {
  const f = o.h;
  if (typeof f === "function") console.log(f(21));
  const m: any = Math.max;
  console.log(m(3, 7));
  const from: any = Array.from;
  console.log(from([1, 2], o.h).join(","));
  const t = o.t;
  t();
}
o.t = () => { throw new Error("boom"); };
main().catch((e: any) => console.log("caught " + e.message));
"#
        ),
        "42\n7\n2,4\ncaught boom"
    );
}

#[test]
fn an_async_body_keeps_the_receiver_of_a_computed_method_call() {
    assert_eq!(
        run_web_source(
            r#"
const o: any = { n: 2 }; o.h = (n: number) => n * 2;
async function main() {
  const a = [1, 2];
  const k = "join";
  const h = "h";
  console.log(a[k]("-") + " " + o[h](21));
}
main();
"#
        ),
        "1-2 42"
    );
}

#[test]
fn an_async_body_passes_a_closure_lookalike_to_a_host_function_unchanged() {
    assert_eq!(
        run_web_source(
            r#"
const fake: any = { funcIdx: 0, captures: [] };
async function main() {
  const of: any = Array.of;
  console.log(typeof of(fake)[0]);
}
main();
"#
        ),
        "object"
    );
}

// Each evaluation of a capturing class expression is its own constructor, and its
// members read the captures of the evaluation that built the instance.
#[test]
fn each_class_expression_evaluation_keeps_its_captures() {
    assert_eq!(
        run_web_source(
            r#"
function make(tag: string) { return class { who() { return tag; } }; }
const A = make("A");
const B = make("B");
const a = new A();
const b = new B();
console.log(a.who() + " " + b.who() + " " + (A === B));
console.log((a instanceof A) + " " + (a instanceof B) + " " + (b instanceof B));
"#
        ),
        "A B false\ntrue false true"
    );
}

#[test]
fn a_class_expression_sees_a_capture_assigned_after_it() {
    assert_eq!(
        run_web_source(
            r#"
function counter(start: number) {
  let n = start;
  const K = class { bump() { n = n + 1; return n; } peek() { return n; } };
  n = start * 10;
  return K;
}
const C1 = counter(1);
const C2 = counter(2);
const c1 = new C1();
console.log(c1.peek() + " " + new C2().peek());
console.log(c1.bump() + " " + new C1().peek() + " " + new C2().bump());
"#
        ),
        "10 20\n11 11 21"
    );
}

// A method read off an instance or off `C.prototype` runs in that class's
// evaluation whatever its `this`, as a JS method closes over its class (#12578).
#[test]
fn a_method_value_runs_in_its_class_expression_evaluation() {
    assert_eq!(
        run_web_source(
            r#"
function make(tag: string) { return class { inst = tag; who() { return tag; } }; }
const A = make("A");
const B = make("B");
const a: any = new A();
const b: any = new B();
console.log(a.who.call(b) + " " + A.prototype.who.call(b) + " " + Reflect.apply(A.prototype.who, b, []));
const borrowed = { who: b.who, inst: "x" };
console.log(borrowed.who() + " " + b.inst);
"#
        ),
        "A A A\nB B"
    );
}

#[test]
fn a_class_declaration_in_a_function_is_fresh_per_call() {
    assert_eq!(
        run_web_source(
            r#"
function declared(t: string) {
  class K { v: number; constructor(v: number) { this.v = v; } who() { return t + this.v; } again() { return new K(this.v + 1); } }
  return new K(1);
}
const x = declared("x");
const y = declared("y");
console.log(x.who() + " " + y.who() + " " + x.again().who() + " " + y.again().who());
function bare() { return class {}; }
console.log(bare() === bare());
"#
        ),
        "x1 y1 x2 y2\nfalse"
    );
}

// `super.x` in a class method reads from the prototype of the method's home: a parent
// class's getter runs on the receiver, a class without `extends` reads Object.prototype,
// and `extends null` has no prototype, so the read throws.
#[test]
fn a_super_property_read_uses_the_home_prototype() {
    assert_eq!(
        run_web_source(
            r#"
class N extends null { m() { return super.x; } }
try { console.log(N.prototype.m.call({})); } catch (e) { console.log(e instanceof TypeError); }
class A { get g() { return "g" + this.k; } }
class B extends A { k = 1; m() { return super.g + " " + typeof super.toString + " " + super.nope; } }
console.log(new B().m());
"#
        ),
        "true\ng1 function undefined"
    );
}

// An evaluation's instance inherits its evaluation and class tag, so it gains no own key,
// whether the class value built it or a member's `new K()` on a later evaluation did.
#[test]
fn an_evaluation_instance_keeps_only_its_declared_own_keys() {
    assert_eq!(
        run_web_source_then(
            r#"
function make(tag: string) { return class { x = 1; who() { return tag; } }; }
function declared(t: string) { class K { v = 2; who() { return t; } again() { return new K(); } } return new K().again(); }
const g: any = globalThis;
const A = make("A");
const B = make("B");
g.a = new A();
g.b = new B();
declared("j");
g.k = declared("k");
console.log(g.a.who() + g.b.who() + g.k.who() + " " + (g.a instanceof A) + " " + (g.a instanceof B) + " " + (g.b instanceof B));
"#,
            r#"
for (const o of [a, b, k]) {
  console.log(Reflect.ownKeys(o).map(String).join(",") + "|" + [...new Headers(o).keys()].join(","));
}
"#
        ),
        "ABk true false true\nx|x\nx|x\nv|v"
    );
}

// Node's `Headers` copies every own key, enumerable or not: an instance or object literal
// built in sync code or in an async (JS) body shows it only declared fields.
#[test]
fn an_instance_has_no_own_class_tag_key() {
    assert_eq!(
        run_web_source_then(
            r#"
class P { x: number; constructor() { this.x = 1; } }
const g: any = globalThis;
g.h = { "Content-Type": "application/json" };
g.p = new P();
async function inBody(): Promise<void> { g.ah = { "X-A": "1" }; g.ap = new P(); }
inBody();
"#,
            r#"
await new Promise((r) => setTimeout(r, 0));
for (const o of [h, p, ah, ap]) {
  console.log(Reflect.ownKeys(o).join(",") + "|" + [...new Headers(o).keys()].join(","));
}
"#
        ),
        "Content-Type|content-type\nx|x\nX-A|x-a\nx|x"
    );
}

// A copy of an instance is an untagged plain object; an object literal's prototype is
// Object.prototype.
#[test]
fn a_copied_instance_is_untagged_and_the_original_still_dispatches() {
    assert_eq!(
        run_web_source_then(
            r#"
class Q { v: number; constructor() { this.v = 2; } get twice(): number { return this.v * 2; } }
const q = new Q();
const c: any = { ...q };
const a: any = Object.assign({}, q);
const g: any = globalThis;
g.q = q; g.c = c; g.lit = { v: 3 };
console.log(JSON.stringify(c) + " " + (c instanceof Q) + " " + (a instanceof Q) + " " + (q instanceof Q) + " " + q.twice);
"#,
            r#"console.log([lit, c, q].map((o) => Object.getPrototypeOf(o) === Object.prototype).join(" "));"#
        ),
        "{\"v\":2} false false true 4\ntrue true false"
    );
}

#[test]
fn an_evaluation_prototype_is_the_one_its_instances_inherit() {
    assert_eq!(
        run_web_source(
            r#"
function make(tag: string) { return class { who() { return tag; } }; }
const A: any = make("A");
A.prototype.extra = 7;
const a: any = new A();
console.log(a.extra + " " + a.who());
"#
        ),
        "7 A"
    );
}

#[test]
fn reflect_apply_takes_an_array_like_argument_list() {
    assert_eq!(
        run_web_source(
            r#"
class A { echo(x: number) { return x; } }
console.log(Reflect.apply(A.prototype.echo, new A(), { 0: 7, length: 1 } as any));
"#
        ),
        "7"
    );
}

// B's setter-only `x` shadows A's getter, so `super.x` from C reads undefined.
#[test]
fn a_super_read_stops_at_a_setter_only_accessor() {
    assert_eq!(
        run_web_source(
            r#"
class A { get x() { return "a"; } }
class B extends A { set x(v: string) {} }
class C extends B { m() { return super.x; } }
console.log(new C().m());
"#
        ),
        "undefined"
    );
}

#[test]
fn an_arrow_function_reads_the_this_it_was_created_with() {
    assert_eq!(
        run_web_source(
            r#"
class B {
  children: any;
  n = 1;
  add = (c: any) => { this.children = c; };
  nested = () => { const f = () => this.n * 10; return f(); };
  later() { return () => this.n + 1; }
  count(a: number) { let c = 0; const inc = (d: number) => { c += this.n + a + d; }; inc(10); return c; }
}
const b = new B();
b.add([7]);
const detached = b.add;
console.log(b.children[0]);
detached("d");
console.log(b.children);
console.log(b.nested());
const f = b.later();
console.log(f());
console.log(b.count(2));
"#
        ),
        "7\nd\n10\n2\n13"
    );
}

#[test]
fn a_function_expression_called_as_a_method_reads_its_receiver() {
    assert_eq!(
        run_web_source(
            r#"
const o = { k: 5, g() { return this.k; } };
console.log(o.g());
const p: any = { name: "p", hi: function () { return "hi " + this.name; } };
const q: any = { name: "q" };
q.hi = p.hi;
console.log(p.hi() + "/" + q.hi());
const counter = { c: 0, inc() { this.c = this.c + 1; return this; } };
counter.inc().inc();
console.log(counter.c);
const r: any = { v: 3, outer() { return () => this.v; } };
console.log(r.outer()());
const holder = { k: 9, g2: o.g };
console.log(holder.g2());
"#
        ),
        "5\nhi p/hi q\n2\n3\n9"
    );
}

#[test]
fn a_parameter_default_reads_the_method_receiver() {
    assert_eq!(
        run_web_source(
            r#"
const o: any = { k: 4, f: function (x = this.k) { return x; } };
console.log(o.f());
console.log(o.f(1));
"#
        ),
        "4\n1"
    );
}

#[test]
fn reflect_apply_hands_this_to_a_function_expression() {
    assert_eq!(
        run_web_source(
            r#"
const hi = function (this: any, p: string) { return p + this.name; };
console.log(Reflect.apply(hi, { name: "o" }, ["hi "]));
"#
        ),
        "hi o"
    );
}

#[path = "class_eval_tests.rs"]
mod class_eval;
