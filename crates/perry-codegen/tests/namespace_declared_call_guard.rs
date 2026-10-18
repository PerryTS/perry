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
    let ir = String::from_utf8(compile_module(&module, options.clone()).unwrap()).unwrap();
    for key in ["level", "enabled", "f"] {
        let spelling: String = key.bytes().map(|b| format!("{b:02x}")).collect();
        assert!(
            ir.contains(&format!("@perry_atom_{spelling} =")),
            "seeded namespace key {key} needs its startup image atom"
        );
    }
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

    assert!(!ir.contains("call void @js_set_call_location("));
    assert!(!ir.contains("invoke void @js_set_call_location("));
    options.debug_locations = true;
    options.module_source = Some(source.to_string());
    let debug_ir = String::from_utf8(compile_module(&module, options).unwrap()).unwrap();
    let generic_blocks: Vec<_> = debug_ir
        .split("\ncompleted.generic_call.")
        .skip(1)
        .collect();
    assert!(!generic_blocks.is_empty());
    for block in generic_blocks {
        let entry = block.split("\n\n").next().unwrap();
        assert!(
            entry.contains("@js_set_call_location("),
            "a generic call must restore its outer source location after arguments: {entry}"
        );
    }
}

#[test]
fn exported_namespace_publishes_a_structural_call_capability() {
    let source = "export namespace Late { export let level=1; export let enabled=false; export function f(v:number) { return level+v; } }";
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(source, "producer.ts", &mut cache).unwrap();
    let module = lower_module(&parsed.module, "producer", "producer.ts").unwrap();
    let (caps, _) = perry_codegen::exported_object_literal_method_facts(&module);
    let cap = caps
        .get("Late")
        .expect("exported namespace has a nominated layout");
    let shape = cap
        .final_shape
        .as_ref()
        .expect("structural namespace contents");
    assert_eq!((shape.key_count, shape.live), (3, 2));
    assert!(cap
        .methods
        .iter()
        .any(|method| method.name == "f" && method.field_index == 2));
}

#[test]
fn imported_uppercase_value_reads_its_method_before_arguments() {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(
        "import { Late, mutate } from './producer'; Late.f(mutate());",
        "consumer.ts",
        &mut cache,
    )
    .unwrap();
    let module = lower_module(&parsed.module, "consumer", "consumer.ts").unwrap();
    let options = CompileOptions {
        emit_ir_only: true,
        output_type: "executable".into(),
        imported_vars: ["Late".to_string()].into_iter().collect(),
        import_function_prefixes: [
            ("Late".to_string(), "producer".to_string()),
            ("mutate".to_string(), "producer".to_string()),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let ir = String::from_utf8(compile_module(&module, options).unwrap()).unwrap();
    let lookup = ir
        .find("call double @js_method_lookup(")
        .expect("split method lookup");
    let argument = ir
        .find("call double @perry_fn_producer__mutate(")
        .expect("observable argument");
    assert!(
        lookup < argument,
        "method lookup must precede argument evaluation"
    );
}
