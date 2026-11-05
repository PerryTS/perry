//! A spread operand whose class declares `*[Symbol.iterator]()` calls the
//! generator body's own entry behind the holder-shape proof and drains that
//! iterator; any other member kind keeps the complete drain only.
use crate::expr::class_field_barrier_tests::ir_opts;

/// The module's IR with its program layouts and static ids computed the way
/// the driver does.
fn ir(source: &str) -> String {
    let ast = perry_parser::parse_typescript(source, "main.ts").unwrap();
    let mut m = perry_hir::lower_module(&ast, "main.ts", "main.ts").unwrap();
    // The driver lowers generators before codegen; the site reads the lowered body.
    perry_transform::transform_generators(&mut m);
    let mut opts = ir_opts();
    opts.program_layouts = std::sync::Arc::new(crate::layout_families::compute(
        [("main.ts", &m)],
        true,
        &Default::default(),
    ));
    let births = crate::module_birth_shapes(&m, opts.clone()).unwrap();
    let ids = crate::assign_static_shape_ids(births.iter().map(|b| &b.shape));
    opts.static_shape_ids = ids.into_iter().collect();
    String::from_utf8(crate::compile_module(&m, opts).unwrap()).expect("LLVM IR should be UTF-8")
}

const SITE: &str = r#"
class Vec4 {
  x: number; y: number; z: number; w: number;
  constructor() { this.x = 0; this.y = 0; this.z = 0; this.w = 1; }
  set(x: any, y: any, z: any, w: any) { this.x = x; this.y = y; this.z = z; this.w = w; return this; }
}
function run(b: Vec4): number {
  const a = new Vec3(1, 2, 3);
  b.set(...a, 1);
  return b.x;
}
console.log(run(new Vec4()));
"#;

#[test]
fn generator_member_spread_calls_the_member_entry_and_drains_it() {
    let source = format!(
        "class Vec3 {{ x: number; y: number; z: number;\n  constructor(x: number, y: number, z: number) {{ this.x = x; this.y = y; this.z = z; }}\n  *[Symbol.iterator]() {{ yield this.x; yield this.y; yield this.z; }}\n}}\n{SITE}"
    );
    let ir = ir(&source);
    let site = ir
        .lines()
        .position(|l| l.contains("call i64 @js_array_spread_append_iterator("))
        .unwrap_or_else(|| panic!("no iterator drain at the spread site:\n{ir}"));
    let lines: Vec<&str> = ir.lines().collect();
    let entry = lines[..site].iter().rev().find(|l| l.contains("__eclo(i64"));
    assert!(
        entry.is_some_and(|l| l.contains("call double @") && l.contains("Vec3")),
        "the drain's iterator is not the member entry's result:\n{ir}"
    );
    assert!(
        ir.contains("call i64 @js_array_spread_append("),
        "a miss keeps the complete drain"
    );
}

#[test]
fn getter_member_spread_keeps_the_complete_drain() {
    let source = format!(
        "class Vec3 {{ x: number; y: number; z: number;\n  constructor(x: number, y: number, z: number) {{ this.x = x; this.y = y; this.z = z; }}\n  get [Symbol.iterator]() {{ const s = this; return function* () {{ yield s.x; }}; }}\n}}\n{SITE}"
    );
    let ir = ir(&source);
    assert!(
        !ir.contains("call i64 @js_array_spread_append_iterator("),
        "a getter member is not a declared generator body:\n{ir}"
    );
}

/// r2b's shape: a module `const` of the class, read by an arrow function. The
/// site's class comes from the binding's inferred type, the guard proves it.
#[test]
fn module_const_operand_in_a_closure_calls_the_member_entry() {
    let source = "class Vec3 {\n  x: number; y: number; z: number;\n  constructor(x = 0, y = 0, z = 0) { this.x = x; this.y = y; this.z = z; }\n  *[Symbol.iterator]() { yield this.x; yield this.y; yield this.z; }\n}\nclass Vec4 {\n  x: number; y: number; z: number; w: number;\n  constructor(x = 0, y = 0, z = 0, w = 1) { this.x = x; this.y = y; this.z = z; this.w = w; }\n  set(x: any, y: any, z: any, w: any) { this.x = x; this.y = y; this.z = z; this.w = w; return this; }\n}\nconst a = new Vec3(1, 2, 3), b = new Vec4();\nconst f = (i: number) => { a.x = i & 7; b.set(...a, 1); return b.x; };\nlet s = 0; for (let i = 0; i < 3; i++) s += f(i);\nconsole.log(s);\n";
    let ir = ir(source);
    assert!(
        ir.contains("call i64 @js_array_spread_append_iterator("),
        "the closure's spread of a module const does not reach the member entry:\n{ir}"
    );
}
