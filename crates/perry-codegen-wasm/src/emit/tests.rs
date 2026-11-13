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
