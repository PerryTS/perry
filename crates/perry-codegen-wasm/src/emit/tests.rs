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
