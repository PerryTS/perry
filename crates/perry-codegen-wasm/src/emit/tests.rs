use std::io::Write;
use std::process::{Command, Stdio};

fn run_web_source(source: &str) -> String {
    run_web_source_then(source, "")
}

/// `run_web_source`, then `after_boot` (plain JS) in the runtime's scope, for an
/// assertion about a runtime helper that TypeScript cannot reach.
fn run_web_source_then(source: &str, after_boot: &str) -> String {
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

// A closure is a `{funcIdx, captures}` record on the host side: `typeof` still answers
// "function", and a call through an object property runs it. A plain object with the
// same keys stays an object.
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

// An async body runs as JS: a closure held in a local is a NaN-boxed handle there, so
// calling it must go through closure dispatch, not a direct JS call on the number.
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
  const t = o.t;
  t();
}
o.t = () => { throw new Error("boom"); };
main().catch((e: any) => console.log("caught " + e.message));
"#
        ),
        "42\n7\ncaught boom"
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
"#
        ),
        "7\nd\n10\n2"
    );
}
