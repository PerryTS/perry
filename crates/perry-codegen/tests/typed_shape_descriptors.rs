use perry_codegen::{compile_module, AppMetadata, CompileOptions};
use perry_hir::{Expr, Function, Interface, InterfaceProperty, Module, ModuleInitKind, Stmt};
use perry_types::{ObjectType, PropertyInfo, Type};

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvVarGuard {
    key: &'static str,
    prev: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: Option<&str>) -> Self {
        let prev = std::env::var_os(key);
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
        Self { key, prev }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.prev {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

fn empty_opts() -> CompileOptions {
    CompileOptions {
        target: None,
        is_entry_module: false,
        non_entry_module_prefixes: Vec::new(),
        import_function_prefixes: std::collections::HashMap::new(),
        import_function_origin_names: std::collections::HashMap::new(),
        import_function_v8_specifiers: std::collections::HashMap::new(),
        import_function_node_submodule: std::collections::HashMap::new(),
        namespace_node_submodules: std::collections::HashMap::new(),
        namespace_v8_specifiers: std::collections::HashMap::new(),
        namespace_member_prefixes: std::collections::HashMap::new(),
        emit_ir_only: true,
        namespace_imports: Vec::new(),
        namespace_reexport_named_imports: std::collections::HashSet::new(),
        imported_classes: Vec::new(),
        imported_enums: Vec::new(),
        imported_async_funcs: std::collections::HashSet::new(),
        type_aliases: std::collections::HashMap::new(),
        imported_func_param_counts: std::collections::HashMap::new(),
        imported_func_has_rest: std::collections::HashSet::new(),
        imported_func_return_types: std::collections::HashMap::new(),
        imported_vars: std::collections::HashSet::new(),
        output_type: "executable".to_string(),
        needs_stdlib: false,
        needs_ui: false,
        needs_geisterhand: false,
        geisterhand_port: 7676,
        needs_js_runtime: false,
        enabled_features: Vec::new(),
        native_module_init_names: Vec::new(),
        js_module_specifiers: Vec::new(),
        bundled_extensions: Vec::new(),
        native_library_functions: Vec::new(),
        i18n_table: None,
        fast_math: false,
        app_metadata: AppMetadata::default(),
        namespace_entries: Vec::new(),
        dynamic_import_path_to_prefix: std::collections::HashMap::new(),
        deferred_module_prefixes: std::collections::HashSet::new(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
    }
}

fn prop(ty: Type) -> PropertyInfo {
    PropertyInfo {
        ty,
        optional: false,
        readonly: false,
    }
}

fn object_type(fields: &[(&str, Type)]) -> Type {
    let mut properties = std::collections::HashMap::new();
    for (name, ty) in fields {
        properties.insert((*name).to_string(), prop(ty.clone()));
    }
    Type::Object(ObjectType {
        name: None,
        properties,
        index_signature: None,
    })
}

fn base_module(name: &str, body: Vec<Stmt>, interfaces: Vec<Interface>) -> Module {
    Module {
        name: name.to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes: Vec::new(),
        interfaces,
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![Function {
            id: 1,
            name: "probe".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Any,
            body,
            is_async: false,
            is_generator: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }],
        init: Vec::new(),
        exported_native_instances: Vec::new(),
        exported_func_return_native_instances: Vec::new(),
        exported_objects: Vec::new(),
        exported_functions: Vec::new(),
        widgets: Vec::new(),
        uses_fetch: false,
        uses_webassembly: false,
        extern_funcs: Vec::new(),
        init_was_unrolled: false,
        has_top_level_await: false,
        init_kind: ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
    }
}

fn ir_for(module: Module) -> String {
    String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap()
}

fn point_module(name: &str, body: Vec<Stmt>) -> Module {
    base_module(name, body, Vec::new())
}

#[test]
fn typed_object_literal_stable_path_installs_pointer_mask_descriptor() {
    let child_ty = object_type(&[("leaf", Type::Number)]);
    let row_iface = Interface {
        id: 1,
        name: "Row".to_string(),
        type_params: Vec::new(),
        extends: Vec::new(),
        properties: vec![
            InterfaceProperty {
                name: "id".to_string(),
                ty: Type::Number,
                optional: false,
                readonly: false,
            },
            InterfaceProperty {
                name: "active".to_string(),
                ty: Type::Boolean,
                optional: false,
                readonly: false,
            },
            InterfaceProperty {
                name: "child".to_string(),
                ty: child_ty.clone(),
                optional: false,
                readonly: false,
            },
        ],
        methods: Vec::new(),
        is_exported: false,
    };
    let module = base_module(
        "typed_shape_literal.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "child".to_string(),
                ty: child_ty,
                mutable: false,
                init: Some(Expr::Object(vec![("leaf".to_string(), Expr::Number(1.0))])),
            },
            Stmt::Let {
                id: 2,
                name: "row".to_string(),
                ty: Type::Named("Row".to_string()),
                mutable: false,
                init: Some(Expr::Object(vec![
                    ("id".to_string(), Expr::Number(7.0)),
                    ("active".to_string(), Expr::Bool(true)),
                    ("child".to_string(), Expr::LocalGet(1)),
                ])),
            },
            Stmt::Return(Some(Expr::LocalGet(2))),
        ],
        vec![row_iface],
    );

    let ir = ir_for(module);
    assert!(
        ir.contains("call i64 @js_object_alloc_with_shape"),
        "fixture should use the stable object-literal shape allocator"
    );
    assert!(
        ir.contains("@perry_typed_obj_shape_mask_"),
        "typed object literal should emit a pointer-mask constant"
    );
    assert!(
        ir.contains("constant [1 x i64] [i64 4]"),
        "only the child slot (slot 2) should be pointer-bearing"
    );

    let mask_call_pos = ir
        .find("ptr @perry_typed_obj_shape_mask_")
        .expect("typed descriptor call should reference the object-literal mask");
    let before_mask_call = &ir[..mask_call_pos];
    let alloc_pos = before_mask_call
        .rfind("call i64 @js_object_alloc_with_shape")
        .expect("descriptor should belong to an object-literal allocation");
    let set_pos = before_mask_call
        .rfind("call void @js_object_set_field")
        .expect("object literal should initialize fields before installing descriptor");
    assert!(alloc_pos < set_pos);
}

#[test]
fn typed_object_literal_pointer_free_descriptor_precedes_dynamic_mutation() {
    let row_ty = object_type(&[("count", Type::Number)]);
    let module = base_module(
        "typed_shape_mutation.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "row".to_string(),
                ty: row_ty,
                mutable: true,
                init: Some(Expr::Object(vec![("count".to_string(), Expr::Number(1.0))])),
            },
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(1)),
                property: "count".to_string(),
                value: Box::new(Expr::String("now-pointer".to_string())),
            }),
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
        Vec::new(),
    );

    let ir = ir_for(module);
    let descriptor_pos = ir
        .find(", i32 1, ptr null, i32 0)")
        .expect("number-only object type should install a pointer-free descriptor");
    let mutation_pos = ir[descriptor_pos..]
        .find("call void @js_object_set_field_by_name")
        .expect("dynamic property mutation should still go through the safe runtime setter");
    assert!(mutation_pos > 0);
}

#[test]
fn unboxed_point_literal_gate_on_emits_raw_setters_and_pointer_free_layout() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _env = EnvVarGuard::set("PERRY_UNBOXED_OBJECT_FIELDS", Some("1"));
    let point_ty = object_type(&[("x", Type::Number), ("y", Type::Number)]);
    let module = point_module(
        "unboxed_point_on.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "p".to_string(),
                ty: point_ty,
                mutable: false,
                init: Some(Expr::Object(vec![
                    ("x".to_string(), Expr::Number(1.5)),
                    ("y".to_string(), Expr::Number(2.5)),
                ])),
            },
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    );

    let ir = ir_for(module);
    assert!(ir.contains("call i64 @js_object_alloc_with_shape"));
    assert!(ir.contains("call void @js_object_set_unboxed_f64_field"));
    assert!(ir.contains("call void @js_gc_init_unboxed_object_layout"));
    assert!(
        ir.contains("i32 2, i64 3, i64 0"),
        "unboxed point layout should install raw f64 slots for x/y and no pointer slots"
    );
    assert!(
        !ir.contains("call void @js_gc_init_typed_shape_layout"),
        "gate-on exact point literals should use the unboxed layout installer"
    );
}

#[test]
fn unboxed_point_literal_gate_off_uses_existing_typed_shape_path() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _env = EnvVarGuard::set("PERRY_UNBOXED_OBJECT_FIELDS", None);
    let point_ty = object_type(&[("x", Type::Number), ("y", Type::Number)]);
    let module = point_module(
        "unboxed_point_off.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "p".to_string(),
                ty: point_ty,
                mutable: false,
                init: Some(Expr::Object(vec![
                    ("x".to_string(), Expr::Number(1.5)),
                    ("y".to_string(), Expr::Number(2.5)),
                ])),
            },
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    );

    let ir = ir_for(module);
    assert!(ir.contains("call i64 @js_object_alloc_with_shape"));
    assert!(ir.contains("call void @js_object_set_field"));
    assert!(ir.contains("call void @js_gc_init_typed_shape_layout"));
    assert!(!ir.contains("call void @js_object_set_unboxed_f64_field"));
    assert!(!ir.contains("call void @js_gc_init_unboxed_object_layout"));
}

#[test]
fn unboxed_point_dynamic_mutation_still_uses_safe_by_name_setter() {
    let _lock = ENV_LOCK.lock().unwrap();
    let _env = EnvVarGuard::set("PERRY_UNBOXED_OBJECT_FIELDS", Some("1"));
    let point_ty = object_type(&[("x", Type::Number), ("y", Type::Number)]);
    let module = point_module(
        "unboxed_point_mutation.ts",
        vec![
            Stmt::Let {
                id: 1,
                name: "p".to_string(),
                ty: point_ty,
                mutable: true,
                init: Some(Expr::Object(vec![
                    ("x".to_string(), Expr::Number(1.0)),
                    ("y".to_string(), Expr::Number(2.0)),
                ])),
            },
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::LocalGet(1)),
                property: "x".to_string(),
                value: Box::new(Expr::String("heap".to_string())),
            }),
            Stmt::Return(Some(Expr::LocalGet(1))),
        ],
    );

    let ir = ir_for(module);
    let layout_pos = ir
        .find("call void @js_gc_init_unboxed_object_layout")
        .expect("fixture should install unboxed layout");
    let mutation_pos = ir[layout_pos..]
        .find("call void @js_object_set_field_by_name")
        .expect("dynamic property mutation should stay on the safe setter");
    assert!(mutation_pos > 0);
}
