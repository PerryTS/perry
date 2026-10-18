use perry_codegen::{assign_static_shape_ids, compile_module, module_birth_shapes, CompileOptions};
use perry_diagnostics::SourceCache;
use perry_hir::lower_module;
use perry_parser::parse_typescript_with_cache;

#[test]
fn namespace_declared_callee_survives_mutation_and_uses_a_spill_guard() {
    let source = include_str!("fixtures/namespace_declared_call_mutations.ts");
    // The namespace portion exercises declaration, replacement, restoration,
    // argument-side mutation, getter and delete; factory fixtures run natively.
    let source = source.split("\nfunction make(").next().unwrap();
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(source, "namespace_guard.ts", &mut cache).unwrap();
    let module = lower_module(&parsed.module, "namespace_guard", "namespace_guard.ts").unwrap();
    let mut options = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".into(),
        disable_constfn_shapes: false,
        ..Default::default()
    };
    let births = module_birth_shapes(&module, options.clone()).unwrap();
    assert!(births
        .iter()
        .any(|b| b.shape.key_count == 3 && b.shape.live == 2));
    options.static_shape_ids = assign_static_shape_ids(births.iter().map(|b| &b.shape))
        .into_iter()
        .collect();
    let ir = String::from_utf8(compile_module(&module, options).unwrap()).unwrap();
    for block in [
        "completed.layout",
        "completed.slot",
        "completed.selected",
        "completed.callee_info",
        "completed.direct",
        "completed.generic_lookup",
    ] {
        assert!(
            ir.contains(block),
            "missing guarded namespace call stage {block}"
        );
    }
}
