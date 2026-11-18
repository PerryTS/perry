//! A `Uint8Array` store whose value is not provably a Number
//! (nanoid's `b[i] = charCodes[b[i] & mask]`, whose read may answer
//! `undefined`) stores a value that IS a Number at runtime through the guarded
//! byte store, and keeps the runtime element store for every other value.
use crate::expr::class_field_barrier_tests::ir_opts;

fn ir(source: &str) -> String {
    let ast = perry_parser::parse_typescript(source, "main.ts").unwrap();
    let m = perry_hir::lower_module(&ast, "main.ts", "main.ts").unwrap();
    String::from_utf8(crate::compile_module(&m, ir_opts()).unwrap())
        .expect("LLVM IR should be UTF-8")
}

/// `(label, body)` of each block of the function whose name contains `name`.
fn blocks(ir: &str, name: &str) -> Vec<(String, String)> {
    let func = ir
        .split("\ndefine ")
        .find(|f| f.lines().next().is_some_and(|l| l.contains(name)))
        .unwrap_or_else(|| panic!("no function `{name}`:\n{ir}"));
    let mut out: Vec<(String, String)> = Vec::new();
    for line in func.lines() {
        if !line.starts_with(char::is_whitespace) && line.ends_with(':') {
            out.push((line.trim_end_matches(':').to_string(), String::new()));
        } else if let Some((_, body)) = out.last_mut() {
            body.push_str(line.trim());
            body.push('\n');
        }
    }
    out
}

const SITE: &str = r#"
function put(b: Uint8Array, src: any, i: number): void { b[i] = src[i]; }
const b = new Uint8Array(4);
put(b, [7, 8, 9, 10], 1);
console.log(b[1]);
"#;

#[test]
fn a_store_of_an_unproven_value_takes_the_byte_store_for_a_number() {
    let ir = ir(SITE);
    let blocks = blocks(&ir, "__put");
    let with = |prefix: &str| -> Vec<&(String, String)> {
        blocks
            .iter()
            .filter(|(l, _)| l.starts_with(prefix))
            .collect()
    };
    let store = with("u8d.set.store");
    assert_eq!(store.len(), 1, "one guarded byte store:\n{blocks:?}");
    assert!(store[0].1.contains("store i8 "), "{:?}", store[0]);
    assert!(!store[0].1.contains(" call "), "{:?}", store[0]);
    // The runtime element store remains, and only on the guard's miss edge.
    let calls: Vec<&String> = blocks
        .iter()
        .filter(|(_, b)| b.contains("@js_typed_array_index_set_dynamic("))
        .map(|(l, _)| l)
        .collect();
    assert_eq!(calls.len(), 1, "{blocks:?}");
    assert!(calls[0].starts_with("u8d.set.slow"), "{calls:?}");
}
