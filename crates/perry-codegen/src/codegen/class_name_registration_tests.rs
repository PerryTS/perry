//! Only defining modules may publish user-visible class names.

use crate::{compile_module, CompileOptions, ImportedClass};
use perry_hir::{Class, Module};

fn imported_class() -> ImportedClass {
    ImportedClass {
        name: "Original".to_string(),
        local_alias: None,
        namespace: None,
        source_prefix: "source_ts".to_string(),
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
        source_class_id: Some(42),
        return_shape_imports: Vec::new(),
        object_literal: None,
    }
}

fn local_class(id: u32, name: &str) -> Class {
    Class {
        id,
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
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    }
}

fn ir(module: &Module, imported: Vec<ImportedClass>) -> String {
    String::from_utf8(
        compile_module(
            module,
            CompileOptions {
                emit_ir_only: true,
                imported_classes: imported,
                ..Default::default()
            },
        )
        .expect("class metadata compiles"),
    )
    .expect("LLVM IR is UTF-8")
}

fn name_registrations(ir: &str) -> Vec<&str> {
    ir.lines()
        .filter(|line| line.contains("call void @js_register_class_name("))
        .collect()
}

#[test]
fn imported_namespace_classes_do_not_register_display_names() {
    let module = Module::new("namespace_consumer.ts");
    let mut imported = imported_class();
    imported.namespace = Some("NS".to_string());
    let ir = ir(&module, vec![imported]);
    assert!(
        ir.contains("call void @js_register_class_id(i32 42)"),
        "imported class must be present in the fixture"
    );
    assert!(
        name_registrations(&ir).is_empty(),
        "the defining module owns the display name: {:?}",
        name_registrations(&ir)
    );
}

#[test]
fn imported_aliases_do_not_register_display_names() {
    let module = Module::new("alias_consumer.ts");
    let mut imported = imported_class();
    imported.local_alias = Some("Renamed".to_string());
    let ir = ir(&module, vec![imported]);
    assert!(
        ir.contains("call void @js_register_class_id(i32 42)"),
        "imported class must be present in the fixture"
    );
    assert!(
        name_registrations(&ir).is_empty(),
        "a local alias must not overwrite the source name: {:?}",
        name_registrations(&ir)
    );
}

#[test]
fn local_classes_register_their_recorded_display_name_once() {
    let mut module = Module::new("class_source.ts");
    let mut class = local_class(42, "InternalUniqueName");
    class.aliases.push("SelfBinding".to_string());
    module.classes.push(class);
    module.class_display_names.insert(42, "Visible".to_string());
    let ir = ir(&module, Vec::new());
    let registrations = name_registrations(&ir);
    assert_eq!(
        registrations.len(),
        1,
        "the local class must still register its name once"
    );
    assert!(registrations[0].contains("i32 42"));
    assert!(
        ir.contains(r#"c"Visible\00""#),
        "the recorded JS display name must be emitted"
    );
}
