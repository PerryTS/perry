//! A body region over exactly one bare access whose key the layout record
//! serves does not pay: the record arm alone is cheaper than the region guard
//! in front of the same access. A body with two bare accesses keeps its region.

use crate::compile_module;
use perry_hir::Module;

fn module(src: &str) -> Module {
    let ast = perry_parser::parse_typescript(src, "main.ts").unwrap();
    perry_hir::lower_module(&ast, "main.ts", "main.ts").unwrap()
}

/// The module's IR with its program layouts computed the way the driver does.
fn ir(src: &str) -> String {
    let m = module(src);
    let opts = crate::CompileOptions {
        emit_ir_only: true,
        output_type: "executable".into(),
        target: Some("x86_64-unknown-linux-gnu".into()),
        program_layouts: std::sync::Arc::new(crate::layout_families::compute(
            [("main.ts", &m)],
            true,
            &Default::default(),
        )),
        ..Default::default()
    };
    String::from_utf8(compile_module(&m, opts).unwrap()).expect("LLVM IR should be UTF-8")
}

fn function_body(ir: &str, name_contains: &str) -> String {
    let at = ir
        .lines()
        .position(|l| l.starts_with("define") && l.contains(name_contains))
        .unwrap_or_else(|| panic!("no `define` containing {name_contains:?} in:\n{ir}"));
    ir.lines()
        .skip(at)
        .take_while(|l| *l != "}")
        .collect::<Vec<_>>()
        .join("\n")
}

fn has_label(body: &str, prefix: &str) -> bool {
    body.lines().any(|l| l.starts_with(prefix))
}

const MIXED: &str = r#"
class A { x: any; y: any; constructor(v: any) { this.x = v; this.y = v + 1; } }
class B { x: any; y: any; w: any; constructor(v: any) { this.x = v; this.y = v + 1; this.w = 0; } }
const arr: any[] = [new A(1), new B(2), { x: 3, y: 4 }, JSON.parse('{"x":5,"y":6}')];
function one(n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) { const o = arr[i & 3]; s += o.x; }
  return s;
}
function two(n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) { const o = arr[i & 3]; s += o.x + o.y; }
  return s;
}
console.log(one(10), two(10));
"#;

#[test]
fn a_one_access_body_has_no_region_and_reads_through_the_record() {
    let ir = ir(MIXED);
    let one = function_body(&ir, "__one");
    assert!(
        !has_label(&one, "rloop."),
        "one record access must not pay for a region:\n{one}"
    );
    assert!(
        has_label(&one, "l2c."),
        "the read goes through the record arm:\n{one}"
    );
}

#[test]
fn a_two_access_body_keeps_its_region() {
    let ir = ir(MIXED);
    let two = function_body(&ir, "__two");
    assert!(
        has_label(&two, "rloop."),
        "two bare accesses keep the region:\n{two}"
    );
}
