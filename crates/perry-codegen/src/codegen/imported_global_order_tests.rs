//! #11409: capability maps must not randomize external-global declarations.
use super::opts::{ImportedObjectLiteralMethod, ObjectLiteralMethodCandidate};
use crate::{compile_module, CompileOptions};
use perry_hir::Module;
use std::collections::HashMap;
use std::sync::Arc;

fn emit() -> String {
    // Fresh RandomStates on every invocation; compiling the same maps twice
    // would preserve their iteration order and miss the regression.
    let mut object = HashMap::new();
    for i in (0..16).rev() {
        let source = format!("producer_{i:02}");
        let shape = format!("perry_class_shape_id_{source}__C");
        let name = format!("method_{i:02}");
        let candidate = ObjectLiteralMethodCandidate {
            class_id: i,
            source_prefix: source,
            source_export_name: "object".into(),
            source_global_id: i,
            shape_id_global: shape,
            method: ImportedObjectLiteralMethod {
                name: name.clone(),
                func_id: i,
                target: format!("perry_closure_{i}"),
                param_count: 0,
                field_index: 0,
            },
        };
        object.insert(name, vec![candidate.clone(), candidate]);
    }
    // Local capabilities must not generate external declarations.
    let mut local = object.values().next().unwrap()[0].clone();
    local.source_prefix = "consumer".into();
    local.shape_id_global = "perry_class_shape_id_consumer__C".into();
    object.insert("local".into(), vec![local]);
    let opts = CompileOptions {
        emit_ir_only: true,
        object_literal_method_candidates: Arc::new(object),
        ..Default::default()
    };
    String::from_utf8(compile_module(&Module::new("consumer"), opts).unwrap()).unwrap()
}

#[test]
fn imported_object_globals_are_stable_and_deduplicated() {
    let first = emit();
    let shapes: Vec<_> = first
        .lines()
        .filter(|line| {
            line.starts_with("@perry_class_shape_id_producer_") && line.contains("external")
        })
        .collect();
    assert_eq!(
        shapes.len(),
        16,
        "all distinct imported shapes must be emitted once"
    );
    assert!(
        shapes.windows(2).all(|pair| pair[0] < pair[1]),
        "shape declarations must be sorted: {shapes:?}"
    );
    let globals: Vec<_> = first
        .lines()
        .filter(|line| line.starts_with("@perry_global_producer_") && line.contains("external"))
        .collect();
    assert_eq!(globals.len(), 16);
    assert!(
        globals.windows(2).all(|pair| pair[0] < pair[1]),
        "producer declarations must be sorted: {globals:?}"
    );
    assert!(!first.contains("@perry_class_shape_id_consumer__C = external"));
    assert!(!first.contains("@perry_global_consumer__"));
    for _ in 0..3 {
        assert_eq!(
            first,
            emit(),
            "fresh capability maps must produce byte-identical IR"
        );
    }
}
