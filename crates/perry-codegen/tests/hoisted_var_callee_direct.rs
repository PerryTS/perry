use perry_codegen::{compile_module, CompileOptions};
use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

fn ir_of(source: &str) -> String {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(source, "hoisted_var_callee.ts", &mut cache).unwrap();
    let module = lower_module(
        &parsed.module,
        "hoisted_var_callee",
        "hoisted_var_callee.ts",
    )
    .unwrap();
    let options = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".into(),
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, options).unwrap()).unwrap()
}

const SHARED: &str = r#"
function make() {
  var keep = function (x: number) { return x + 1; };
  var swap = function (x: number) { return x + 2; };
  swap = function (x: number) { return x + 3; };
  return { a: () => keep(1), b: () => swap(1) };
}
"#;

#[test]
fn captured_hoisted_var_closure_keeps_an_identity_guarded_direct_arm() {
    let ir = ir_of(SHARED);
    assert!(
        ir.contains("closure_direct.fast"),
        "a never-reassigned var callee resolves to its closure entry"
    );
    assert!(
        ir.contains("closure_direct.guard_call") || ir.contains("closure_direct.inline_probe"),
        "a var callee may still be undefined when called early, so the identity check stays"
    );
}

#[test]
fn reassigned_hoisted_var_closure_gets_no_direct_arm() {
    let ir = ir_of(&SHARED.replace(
        "var keep = function (x: number) { return x + 1; };",
        "var keep = function (x: number) { return x + 1; };\n  keep = function (x: number) { return x + 4; };",
    ));
    assert!(
        !ir.contains("closure_direct.fast"),
        "a var callee that is assigned again names no single closure"
    );
}
