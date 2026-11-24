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
  insertBefore(child, next) { this.children.splice(this.children.indexOf(next), 0, child); return child; }
  removeChild(child) { this.children.splice(this.children.indexOf(child), 1); return child; }
  contains(child) { return this === child || this.descendants().includes(child); }
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
    run_web_modules_with_dom(&[("splice", source)], dom, after_boot)
}

/// Compiles `(name, source)` modules, importable as `./<name>`, into one program
/// whose entry is the last module, and runs it as `run_web_source` does.
fn run_web_modules(modules: &[(&str, &str)]) -> String {
    run_web_modules_with_dom(modules, STUB_DOM, "")
}

fn run_web_modules_with_dom(modules: &[(&str, &str)], dom: &str, after_boot: &str) -> String {
    let modules: Vec<_> = modules
        .iter()
        .map(|(name, source)| {
            let path = format!("{name}.ts");
            let ast = perry_parser::parse_typescript(source, &path).unwrap();
            let mut module = perry_hir::lower::lower_module(&ast, name, &path).unwrap();
            // The HIR transforms `perry --target web` runs before wasm emission.
            perry_transform::inline_finally_into_returns(&mut module);
            perry_transform::transform_generators_without_private_frames(&mut module);
            (path, module)
        })
        .collect();
    let entry = modules.last().unwrap().1.name.clone();
    let html = crate::compile_modules_to_wasm_html(&modules, &entry, false).unwrap();
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
        run_web_source(
            "const a = [1, 2]; let n = 0; a.splice(1, 0, ++n); console.log(a.join(',')); console.log(n);"
        ),
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

// `push` with two or more arguments appends every one in order and returns the new length,
// on a typed and on an `any` receiver. Expected output is Node's.
#[test]
fn push_with_several_arguments_appends_every_one() {
    assert_eq!(
        run_web_source(
            r#"
const a: number[] = [1, 2, 3];
const n = a.push(4, 5);
console.log(n + " " + a.join(","));
const b: any = [1];
let k = 0;
console.log(b.push("x", ++k, "z", ++k) + " " + b.join(","));
const c: number[] = [];
c.push(7);
console.log(c.length + " " + c.join(","));
"#
        ),
        "5 1,2,3,4,5\n5 1,x,1,z,2\n1 7"
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

// A spread takes any iterable, and `forEach`, which is dispatched by name, runs over a
// Set or a Map as well as an array. Expected output is Node's.
#[test]
fn spread_and_for_each_visit_a_set_or_a_maps_entries() {
    assert_eq!(
        run_web_source(
            r#"
function spreads() { const s = new Set<number>([1, 2, 3]); const a: any = new Set(["x"]); const m = new Map<string, number>([["a", 1], ["b", 2]]); return [...s].join(",") + " " + [0, ...a].join(",") + " " + [...m].map((e) => e[0] + e[1]).join(",") + " " + [...m.keys()].join(",") + [...m.values()].join(","); }
function each() { const out: string[] = []; const s = new Set<string>(["x", "y"]); s.forEach((v, k, set) => out.push(v + k + set.size)); const m: any = new Map([["a", 1], ["b", 2]]); m.forEach((v: number, k: string) => out.push(k + "=" + v)); return out.join(","); }
const big = new Set<number>(); for (let i = 0; i < 200000; i++) big.add(i);
console.log(spreads() + " | " + each() + " | " + [..."ab"].join("-") + " | " + [...big].length);
"#
        ),
        "1,2,3 0,x a1,b2 a,b1,2 | xx2,yy2,a=1,b=2 | a-b | 200000"
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

// A call through a closure or function value runs the callee with every argument, for any
// argument count, evaluates each argument once, and a void callee yields `undefined`.
// Expected output is Node's.
#[test]
fn a_dynamic_call_passes_every_argument_once() {
    assert_eq!(
        run_web_source(
            r#"
const f4: any = (a: number, b: number, c: number, d: number) => "" + a + b + c + d;
const f5: any = (a: number, b: number, c: number, d: number, e: number) => "" + a + b + c + d + e;
function g4(a: number, b: number, c: number, d: number) { return "" + a + b + c + d; }
const gv: any = g4;
function viaParam(fn: (a: number, b: number, c: number, d: number) => number) { return fn(1, 2, 3, 4); }
console.log("label: " + f4(1, 2, 3, 4) + " " + f5(1, 2, 3, 4, 5) + " " + gv(5, 6, 7, 8) + " " + viaParam((a, b, c, d) => a + b + c + d));
let n = 0;
function tick(): number { n++; return n; }
const a1: any = (x: number) => x;
const a2: any = (x: number, y: number) => x + y;
const t1: (x: number) => number = (x) => x;
const counts: number[] = [];
n = 0; a1(tick()); counts.push(n);
n = 0; a2(tick(), tick()); counts.push(n);
n = 0; f4(tick(), 0, 0, 0); counts.push(n);
n = 0; t1(tick()); counts.push(n);
console.log(counts.join(","));
let sunk = 0;
function sink(a: number, b?: number, c?: number, d?: number): void { sunk += a; }
const sv: any = sink;
console.log(sv(1) + " " + sv(1, 2, 3, 4) + " " + sunk);
"#
        ),
        "label: 1234 12345 5678 10\n1,2,1,1\nundefined undefined 2"
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
fn disclosure_is_details_with_the_title_as_summary() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { Disclosure, Text, widgetAddChild } from "perry/ui";
const more = Disclosure("More details");
widgetAddChild(more, Text("gloves"));
widgetAddChild(more, Text("raincoat"));
"#,
            r#"
const details = [...uiHandles.values()].find(e => e.tagName === "DETAILS");
console.log(details.children.map(c => c.tagName + ":" + c.textContent).join(" "));
"#
        ),
        "SUMMARY:More details SPAN:gloves SPAN:raincoat"
    );
}

// The title of a Section or a Disclosure is not one of its children: indices start at
// the first child, and clearing the children keeps the title.
#[test]
fn a_section_or_disclosure_title_is_not_a_child() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { Disclosure, Section, Text, widgetAddChild, widgetClearChildren, widgetRemoveChild, widgetReorderChild } from "perry/ui";
for (const make of [(t: string) => Section(t), (t: string) => Disclosure(t)]) {
  const box = (title: string) => {
    const w = make(title);
    for (const t of ["a", "b", "c"]) widgetAddChild(w, Text(t));
    return w;
  };
  widgetReorderChild(box("0 to 1"), 0, 1);
  widgetReorderChild(box("2 to 0"), 2, 0);
  widgetReorderChild(box("0 to 9"), 0, 9);
  widgetReorderChild(box("3 to 0"), 3, 0);
  const removed = make("removed");
  const gone = Text("a");
  widgetAddChild(removed, gone);
  widgetAddChild(removed, Text("b"));
  widgetRemoveChild(removed, gone);
  widgetClearChildren(box("cleared"));
  const refilled = box("refilled");
  widgetClearChildren(refilled);
  widgetAddChild(refilled, Text("d"));
}
"#,
            r#"
for (const tag of ["FIELDSET", "DETAILS"]) {
  const boxes = [...uiHandles.values()].filter(e => e.tagName === tag);
  console.log(boxes.map(e => e.children.map(c => c.textContent).join(" ")).join(", "));
}
"#
        ),
        ["0 to 1 b a c, 2 to 0 c a b, 0 to 9 b c a, 3 to 0 a b c, removed b, cleared, refilled d";
            2]
            .join("\n")
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

// The web DatePicker reports the ISO `yyyy-MM-dd` string the native backends report;
// an out-of-range year or month falls back to 2026-01, and a day past the month's end
// is the month's last.
#[test]
fn date_picker_reports_and_sets_an_iso_date() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { DatePicker, datePickerGetSelectedDate, datePickerSetDate } from "perry/ui";
const picker = DatePicker(2026, 11, (iso: string) => { console.log("changed " + iso + " " + typeof iso); });
console.log(datePickerGetSelectedDate(picker));
datePickerSetDate(picker, 2027, 2, 9);
console.log(datePickerGetSelectedDate(picker));
datePickerSetDate(picker, 2028, 2, 31);
console.log(datePickerGetSelectedDate(picker));
const fallback = DatePicker(0, 13, (iso: string) => {});
console.log(datePickerGetSelectedDate(fallback));
"#,
            r#"
const input = [...uiHandles.values()].find(e => e.tagName === "INPUT" && e.type === "date");
input.value = "2026-11-12";
input.dispatch("change");
"#
        ),
        "2026-11-01\n2027-02-09\n2028-02-29\n2026-01-01\nchanged 2026-11-12 string"
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

// `super.x` in a static method reads the parent's static methods, as `Parent.x`
// does, and keeps the subclass as `this`. Expected output is Node's.
#[test]
fn super_in_a_static_method_reaches_the_parents_static_methods() {
    assert_eq!(
        run_web_source(
            r#"
class A { static f() { return "f"; } static g() { return "g:" + this.name; } }
class B extends A { static h() { return super.f() + "h"; } static i() { return super.g(); } }
class C extends B {}
console.log("B.f(): " + B.f());
console.log("C.f(): " + C.f());
console.log("B.h() (super.f()): " + B.h());
console.log("B.i() (super.g() keeps this): " + B.i());
console.log("C.h(): " + C.h());
"#
        ),
        "B.f(): f\nC.f(): f\nB.h() (super.f()): fh\nB.i() (super.g() keeps this): g:B\nC.h(): fh"
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

#[path = "tests_more.rs"]
mod more;
