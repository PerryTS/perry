use std::io::Write;
use std::process::{Command, Stdio};

fn run_web_source(source: &str) -> String {
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
