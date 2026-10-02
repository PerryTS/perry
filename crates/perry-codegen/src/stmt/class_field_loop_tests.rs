//! P8 replacement IR checks for the original literal/module-bound and strict
//! class increment shapes. Runtime, hostile-value, and cost acceptance remains
//! separate and must use the unchanged original TypeScript fixtures.

use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{
    BinaryOp, Class, ClassField, CompareOp, Expr, Module, ModuleInitKind, Stmt, UpdateOp,
};

fn ir_opts() -> CompileOptions {
    CompileOptions {
        static_shape_ids: Vec::new(),
        program_class_shape_ids: Default::default(),
        target: None,
        is_entry_module: true,
        non_entry_module_prefixes: Vec::new(),
        thread_literal_module_prefixes: Vec::new(),
        nextjs_path_init_modules: Vec::new(),
        import_function_prefixes: std::collections::HashMap::new(),
        import_function_ffi_aliases: std::collections::HashMap::new(),
        import_function_origin_names: std::collections::HashMap::new(),
        import_function_v8_specifiers: std::collections::HashMap::new(),
        import_function_node_submodule: std::collections::HashMap::new(),
        namespace_node_submodules: std::collections::HashMap::new(),
        namespace_v8_specifiers: std::collections::HashMap::new(),
        namespace_member_prefixes: std::collections::HashMap::new(),
        namespace_member_origin_names: std::collections::HashMap::new(),
        emit_ir_only: true,
        verify_native_regions: false,
        disable_buffer_fast_path: false,
        namespace_imports: Vec::new(),
        namespace_member_nested: Vec::new(),
        constructor_param_counts: Default::default(),
        imported_classes: Vec::new(),
        short_spread_method_candidates: std::sync::Arc::default(),
        object_literal_method_candidates: std::sync::Arc::default(),
        imported_enums: Vec::new(),
        imported_async_funcs: std::collections::HashSet::new(),
        type_aliases: std::collections::HashMap::new(),
        imported_func_param_counts: std::collections::HashMap::new(),
        imported_func_has_rest: std::collections::HashSet::new(),
        imported_func_synthetic_arguments: std::collections::HashSet::new(),
        imported_func_return_types: std::collections::HashMap::new(),
        imported_vars: std::collections::HashSet::new(),
        output_type: "executable".to_string(),
        needs_stdlib: false,
        program_is_synchronous: false,
        needs_ui: false,
        needs_geisterhand: false,
        geisterhand_port: 7676,
        enabled_features: Vec::new(),
        native_module_init_names: Vec::new(),
        js_module_specifiers: Vec::new(),
        bundled_extensions: Vec::new(),
        native_library_functions: Vec::new(),
        i18n_table: None,
        fast_math: false,
        fp_contract_mode: crate::FpContractMode::Off,
        app_metadata: AppMetadata::default(),
        namespace_entries: Vec::new(),
        dynamic_import_path_to_prefix: std::collections::HashMap::new(),
        deferred_module_prefixes: std::collections::HashSet::new(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        debug_locations: false,
        module_source: None,
        debug_source_line_offset: 0,
    }
}

fn counter_class() -> Class {
    let mut class = Class {
        id: 101,
        name: "Counter".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![ClassField {
            name: "value".to_string(),
            key_expr: None,
            ty: Type::Number,
            init: None,
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        }],
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
        alloc_width_hint: 0,
        specialized_from: None,
    };
    class.constructor = Some(assigning_ctor(&class.fields));
    class
}

/// The constructor a fixture class carries: `constructor(f1, ..) { this.f1 =
/// f1; .. }`, the shape `perry-hir`'s `mint_anon_shape_class` synthesizes.
/// Its `number` fields are written before anything can observe the instance,
/// so they are born on `F64` lanes (`lower_call::birth_lanes`); a class that
/// never writes a `number` field during construction must answer `undefined`
/// for it and is born `Any` there.
fn assigning_ctor(fields: &[ClassField]) -> perry_hir::Function {
    let params: Vec<perry_hir::Param> = fields
        .iter()
        .enumerate()
        .map(|(i, f)| perry_hir::Param {
            id: 9000 + i as u32,
            name: f.name.clone(),
            ty: f.ty.clone(),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        })
        .collect();
    let body = params
        .iter()
        .map(|p| {
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: p.name.clone(),
                value: Box::new(Expr::LocalGet(p.id)),
            })
        })
        .collect();
    perry_hir::Function {
        id: 9000,
        name: "constructor".to_string(),
        type_params: Vec::new(),
        params,
        return_type: Type::Void,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    }
}

/// `counter.value = counter.value + 1`, in the shape the inliner leaves behind
/// for `counter.increment()` at module scope: a sloppy `PutValueSet` whose
/// target and receiver are the same local.
fn bump_stmt(recv_id: u32, strict: bool) -> Stmt {
    Stmt::Expr(Expr::PutValueSet {
        target: Box::new(Expr::LocalGet(recv_id)),
        key: Box::new(Expr::String("value".to_string())),
        value: Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(recv_id)),
                property: "value".to_string(),
                byte_offset: 0,
            }),
            right: Box::new(Expr::Integer(1)),
        }),
        receiver: Box::new(Expr::LocalGet(recv_id)),
        strict,
    })
}

/// The module-init shape of `benchmarks/suite/09_method_calls.ts` after
/// inlining: `const c = new Counter(); for (let i = 0; i < <bound>; i++) c.value
/// = c.value + 1;`.
fn method_calls_module(bound: Expr, extra_init: Vec<Stmt>, strict: bool) -> Module {
    let mut m = Module::new("class_field_loop.ts");
    m.classes = vec![counter_class()];
    let mut init = extra_init;
    init.push(Stmt::Let {
        id: 1,
        name: "c".to_string(),
        ty: Type::Named("Counter".to_string()),
        mutable: false,
        init: Some(Expr::New {
            class_name: "Counter".to_string(),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
            cap_args_appended: 0,
        }),
    });
    init.push(Stmt::For {
        init: Some(Box::new(Stmt::Let {
            id: 7,
            name: "i".to_string(),
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Integer(0)),
        })),
        condition: Some(Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(Expr::LocalGet(7)),
            right: Box::new(bound),
        }),
        update: Some(Expr::Update {
            id: 7,
            op: UpdateOp::Increment,
            prefix: false,
        }),
        body: vec![bump_stmt(1, strict)],
    });
    m.init = init;
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn emit(m: &Module) -> String {
    String::from_utf8(compile_module(m, ir_opts()).unwrap()).expect("LLVM IR should be UTF-8")
}

/// P8: a numeric class-field module loop must use the generic guarded F/G body.
/// Runtime route/shape/store attribution is checked separately by the isolated
/// executable. These IR assertions never certify retained-tier cost parity.
fn assert_versioned_loop_lowered(ir: &str, what: &str) {
    for label in [
        "rloop.guard.",
        "rloop.fast",
        "rloop.join",
        "rloop.version.plain",
    ] {
        assert!(
            ir.contains(label),
            "{what}: missing generic replacement `{label}`"
        );
    }
    for label in [
        "class_field.loop.",
        "class_field_loop.",
        "class_field_loop_store.",
        "for.class_field_fast",
        "for.class_field_slow",
    ] {
        assert!(!ir.contains(label), "{what}: legacy tier remains `{label}`");
    }
    let fast = ir
        .lines()
        .skip_while(|line| !line.starts_with("rloop.fast"))
        .take_while(|line| !line.starts_with("rloop.join"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        fast.contains("load double")
            && fast.contains("fadd double")
            && fast.contains("store double")
            && ir
                .lines()
                .any(|line| line.contains("br i1 ") && line.contains("label %rloop.version.split")),
        "{what}: reachable generic F must read, add and commit the increment"
    );
    assert!(
        !fast.contains("@js_class_field_")
            && !fast.contains("@js_object_get_field")
            && !fast.contains("@js_number_coerce")
            && !fast.contains("@js_dynamic_string_or_number_add")
            && !fast.contains("@js_put_value"),
        "{what}: typed class read must consume exact R, not its ordinary guard"
    );
    assert!(
        ir.lines()
            .any(|line| line.contains("call i64 @js_region_loop_prime(")
                && line.contains("i32 1, i32 0, i32 1)")),
        "{what}: increment must request stored=1, boxed=0 and R=1"
    );
}

/// The exact `09_method_calls` shape: an integer-literal bound.
#[test]
fn class_field_versioned_loop_fires_for_literal_bound() {
    let ir = emit(&method_calls_module(
        Expr::Integer(10_000_000),
        Vec::new(),
        false,
    ));
    assert_versioned_loop_lowered(&ir, "literal bound");
}

/// The benchmark as actually written: the bound is a module-scope
/// `const ITERATIONS = 10000000`. Under repsel Phase 1 that const is a
/// canonical-i32 local with no `ctx.locals` entry either, so it exercises the
/// bound half of the admission fix independently of the counter half.
#[test]
fn class_field_versioned_loop_fires_for_module_scope_counter() {
    let iterations = Stmt::Let {
        id: 0,
        name: "ITERATIONS".to_string(),
        ty: Type::Number,
        mutable: false,
        init: Some(Expr::Integer(10_000_000)),
    };
    let ir = emit(&method_calls_module(
        Expr::LocalGet(0),
        vec![iterations],
        false,
    ));
    assert_versioned_loop_lowered(&ir, "module-scope const bound");
}

/// STRICT module scope takes a different ordinary store lowering
/// (`put_value_static_property_fast_path` → `property_set::lower`). Both modes
/// must consume the same guarded region store proof in F.
#[test]
fn class_field_versioned_loop_fires_in_strict_mode() {
    let ir = emit(&method_calls_module(
        Expr::Integer(10_000_000),
        Vec::new(),
        true,
    ));
    assert_versioned_loop_lowered(&ir, "strict mode");
}

/// Replacement's scoped suppression must not erase the pre-existing receiver
/// proof after F/G joins. The subsequent read still uses a direct Ptr<Shape>
/// load, but keeps the ordinary boxed-value/coercion check: R must not escape.
#[test]
fn class_loop_replacement_restores_straight_line_receiver_proof() {
    let mut module = method_calls_module(Expr::Integer(200), Vec::new(), false);
    module.init.push(Stmt::Expr(Expr::Binary {
        op: BinaryOp::Mul,
        left: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(1)),
            property: "value".to_string(),
            byte_offset: 0,
        }),
        right: Box::new(Expr::Integer(2)),
    }));
    let ir = emit(&module);
    assert_versioned_loop_lowered(&ir, "subsequent read");
    let post = ir
        .lines()
        .skip_while(|line| !line.starts_with("rloop.version.merge"))
        .skip(1)
        .take_while(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        post.contains("load double")
            && post.contains("label %ptr_shape_get_number.coerce")
            && !post.contains("@js_class_field_")
            && !post.contains("class_field_inline"),
        "post-loop shape proof must return without leaking Number R:\n{post}"
    );
}

/// The removed twin admitted only a single expression. Region admission must
/// come from its own effect and representation proof, without keeping that
/// old syntactic matcher as a second authority.
#[test]
fn numeric_class_region_accepts_multiple_commits() {
    let mut module = method_calls_module(Expr::Integer(200), Vec::new(), false);
    let body = module
        .init
        .iter_mut()
        .find_map(|stmt| match stmt {
            Stmt::For { body, .. } => Some(body),
            _ => None,
        })
        .expect("fixture must contain the original increment loop");
    let increment = body[0].clone();
    body.push(increment);
    let ir = emit(&module);
    assert_versioned_loop_lowered(&ir, "multiple numeric commits");
    let fast = ir
        .lines()
        .skip_while(|line| !line.starts_with("rloop.fast"))
        .take_while(|line| !line.starts_with("rloop.join"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        fast.matches("fadd double").count(),
        2,
        "both increment expressions must consume the same guarded numeric lane"
    );
}
