//! A class that extends nothing never emits a super-construct, even when an
//! imported class shares its name (#12889).
use crate::{compile_module, CompileOptions, ImportedClass};
use perry_hir::{Class, Module};

fn no_heritage_class(name: &str) -> Class {
    Class {
        id: 1,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        computed_members: Vec::new(),
        decorators: Vec::new(),
        is_exported: true,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    }
}

/// An entry whose name collides with a class the module declares, carrying no
/// own constructor — what a type-only import cycle produces.
fn same_named_import(name: &str) -> ImportedClass {
    ImportedClass {
        name: name.to_string(),
        local_alias: None,
        namespace: None,
        source_prefix: "unit_ts".to_string(),
        constructor_param_count: 0,
        has_own_constructor: false,
        constructor_has_rest: false,
        constructor_has_synthetic_arguments: false,
        has_instance_fields: false,
        method_names: Vec::new(),
        proven_this_method_names: Vec::new(),
        proven_this_tower_method_names: Vec::new(),
        method_return_types: Vec::new(),
        method_param_counts: Vec::new(),
        method_has_rest: Vec::new(),
        method_has_synthetic_arguments: Vec::new(),
        method_arguments_length_only: Vec::new(),
        static_field_names: Vec::new(),
        static_method_names: Vec::new(),
        static_method_return_types: Vec::new(),
        static_method_param_counts: Vec::new(),
        static_method_has_rest: Vec::new(),
        static_method_has_user_rest: Vec::new(),
        static_method_has_synthetic_arguments: Vec::new(),
        getter_names: Vec::new(),
        getter_return_types: Vec::new(),
        setter_names: Vec::new(),
        parent_name: None,
        field_names: Vec::new(),
        field_types: Vec::new(),
        source_class_id: Some(7),
        return_shape_imports: Vec::new(),
        object_literal: None,
    }
}

#[test]
fn a_class_extending_nothing_emits_no_super_construct_despite_a_same_named_import() {
    // A type-only import cycle registers an imported class under the name of a
    // class this module DECLARES. `requires_live` keys that map on the bare
    // class name, so the collision reported live heritage for a class with no
    // parent at all, and the emitted super-construct threw
    // `Super constructor is not a constructor` at run time.
    let mut module = Module::new("main.ts");
    module.classes = vec![no_heritage_class("World")];

    let ir = String::from_utf8(
        compile_module(
            &module,
            CompileOptions {
                emit_ir_only: true,
                imported_classes: vec![same_named_import("World")],
                ..Default::default()
            },
        )
        .expect("module compiles"),
    )
    .expect("LLVM IR is UTF-8");

    // The extern is always DECLARED in the preamble; only a CALL to it means
    // the class was lowered as if its heritage could change.
    let calls: Vec<&str> = ir
        .lines()
        .filter(|line| line.contains("call double @js_class_value_super_construct_site("))
        .collect();
    assert!(
        calls.is_empty(),
        "a class with no heritage must not reach the live-heritage super \
         dispatcher, got: {calls:?}"
    );
}
