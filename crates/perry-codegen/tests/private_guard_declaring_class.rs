//! Regression: a private-member brand guard must use the DECLARING CLASS'S
//! UNIQUE id, not a name lookup.
//!
//! `js_private_guard` brand-checks the receiver against a declaring class id.
//! Codegen used to derive that id by resolving `Expr::PrivateGuard.class_name`
//! through `class_ids` — a `name -> id` map built by
//! `hir.classes.map(|c| (c.name, c.id)).collect()`, i.e. last-writer-wins.
//! Minified programs reuse class names, so two distinct classes with the same
//! name collapse to one entry: a `this.#x` inside the class that LOSES the
//! collision then brand-checks against the OTHER same-named class, whose id is
//! not in the receiver's class-id chain, and throws
//!   `Cannot access private member from an object whose class did not declare it`
//! for a legal access.
//!
//! A faithful end-to-end repro needs the specific duplicate-name shape a
//! minifier produces, so this asserts the codegen contract directly on the
//! emitted IR: `Expr::PrivateGuard` now carries the declaring class's unique
//! id, and codegen passes THAT to `js_private_guard` rather than the collided
//! `class_ids[name]`.

use perry_codegen::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Class, Expr, Function, Module, ModuleInitKind, Stmt};

fn ir_opts() -> CompileOptions {
    CompileOptions {
        is_entry_module: true,
        emit_ir_only: true,
        output_type: "executable".to_string(),
        ..Default::default()
    }
}

fn class(id: u32, name: &str) -> Class {
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
        computed_members: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
    }
}

fn module_with(classes: Vec<Class>, body: Vec<Stmt>) -> Module {
    Module {
        name: "private_guard_declaring_class.ts".to_string(),
        imports: Vec::new(),
        exports: Vec::new(),
        classes,
        interfaces: Vec::new(),
        type_aliases: Vec::new(),
        enums: Vec::new(),
        globals: Vec::new(),
        functions: vec![Function {
            id: 1,
            name: "probe".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Number,
            body,
            is_async: false,
            is_generator: false,
            is_strict: false,
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
        script_global_functions: Vec::new(),
        references_global_this: false,
        annexb_global_undefined_names: Vec::new(),
        widgets: Vec::new(),
        uses_fetch: false,
        uses_webassembly: false,
        extern_funcs: Vec::new(),
        init_was_unrolled: false,
        has_top_level_await: false,
        init_kind: ModuleInitKind::Eager,
        async_step_closures: std::collections::HashSet::new(),
        closure_display_names: std::collections::HashMap::new(),
        class_display_names: std::collections::HashMap::new(),
        closure_source_text: std::collections::HashMap::new(),
        async_generator_funcs: std::collections::HashSet::new(),
        gen_param_prologue_len: std::collections::HashMap::new(),
    }
}

#[test]
fn private_guard_uses_declaring_class_id_not_name_collided_id() {
    // Two DISTINCT classes both named "Box": ids 5 and 7. `class_ids` is a
    // name-keyed map, so "Box" collapses to a single entry (id 7 — the last
    // one collected). The guard below declares class id 5 (the OTHER "Box").
    let classes = vec![class(5, "Box"), class(7, "Box")];
    let guard = Expr::PrivateGuard {
        class_name: "Box".to_string(),
        class_id: 5,
        field_name: "#v".to_string(),
        kind: 0, // field
        op: 0,   // instance read
        object: Box::new(Expr::Integer(0)),
    };
    let ir = String::from_utf8(
        compile_module(&module_with(classes, vec![Stmt::Expr(guard)]), ir_opts()).unwrap(),
    )
    .expect("LLVM IR should be UTF-8");

    let call = ir
        .lines()
        .find(|l| l.contains("call") && l.contains("@js_private_guard"))
        .unwrap_or_else(|| panic!("no js_private_guard CALL emitted:\n{ir}"));

    // The brand must use the declaring class's own id (5), carried on the
    // PrivateGuard node — NOT `class_ids["Box"]` (7), which resolves to the
    // wrong same-named class.
    assert!(
        call.contains("i32 5"),
        "brand must use the declaring class id 5 carried on the node:\n{call}"
    );
    assert!(
        !call.contains("i32 7"),
        "brand must NOT use the name-collided class_ids[\"Box\"] id 7:\n{call}"
    );
}
