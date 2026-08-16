//! #8040: a class method whose body reads `arguments` must receive EVERY
//! passed argument in that array — not only the ones past its declared
//! parameters.
//!
//! `arguments`-synthesis (#677) appends a hidden trailing parameter to such a
//! method and marks it `is_rest`, which is exactly how a user `...rest` is
//! spelled. Every class-method call site keyed off that single bit, so it
//! bundled from `declared - 1` — the offset a user rest wants — and
//! `m(a, b) { arguments }` called as `m(1, 2, 3)` received `arguments === [3]`.
//! The freestanding-function path (`lower_call/func_ref.rs`) has always emitted
//! the other shape: bundle from argument 0 and mark the array.
//!
//! This is an IR-census test on the CALL SITE, which is where the defect lived.
//! Both halves matter and both are asserted:
//!
//!   * the array is filled from argument 0 (three `js_array_push_f64` into the
//!     bundle for a three-argument call to a two-parameter method), and
//!   * it is marked with `js_array_mark_arguments_object`, without which the
//!     callee's `arguments` is an ordinary Array.
//!
//! The negative half asserts the fix did not widen: a method with a real
//! `...rest` and NO `arguments` read must still bundle from `declared - 1`, and
//! must NOT be marked. A change that simply marked every rest array, or always
//! bundled from zero, passes the positive test and fails this one.
//!
//! Why this shape, and not a smaller unit test on the predicate: the predicate
//! answering correctly is not the property that broke. What broke is the
//! emitted call, and a resolver that returns the right pair while the emission
//! ignores it is precisely the regression this must catch.
//!
//! Field evidence for the severity: Next.js bundles OpenTelemetry's
//! `NoopTracer.startActiveSpan`, whose first statement is
//! `if (arguments.length < 2) return;`. Under the conflation that guard fired
//! on every well-formed three-argument call, so `tracer.trace()` returned
//! `undefined` without ever invoking its callback — a production App Route
//! resolved its handler having never entered `routeModule.handle`, and answered
//! with an empty 200.

use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{ArgumentsObjectMeta, Class, Expr, Function, Module, ModuleInitKind, Param, Stmt};

// The CALL, not the name: `runtime_decls` emits a `declare` for this helper
// in every module, so a bare substring match is true even when nothing
// marks anything — a negative written against the name alone cannot fail.
const MARK: &str = "call i64 @js_array_mark_arguments_object(";
const PUSH: &str = "js_array_push_f64";

const A_ID: u32 = 1;
const B_ID: u32 = 2;
const TAIL_ID: u32 = 3;
const RECV_ID: u32 = 4;

fn ir_opts() -> CompileOptions {
    CompileOptions {
        target: None,
        is_entry_module: true,
        non_entry_module_prefixes: Vec::new(),
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
        imported_classes: Vec::new(),
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

/// The synthesized `arguments` parameter #677 appends: trailing, `is_rest`, and
/// the ONLY parameter carrying `arguments_object`.
fn synthetic_arguments_param() -> Param {
    Param {
        id: TAIL_ID,
        name: "arguments".to_string(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: true,
        arguments_object: Some(ArgumentsObjectMeta {
            strict: true,
            simple_parameters: false,
            mapped_parameter_ids: Vec::new(),
            restricted_callee: true,
        }),
    }
}

/// A user `...rest`. Same `is_rest`, no `arguments_object` — the pair the call
/// site has to tell apart.
fn user_rest_param() -> Param {
    Param {
        id: TAIL_ID,
        name: "rest".to_string(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: true,
        arguments_object: None,
    }
}

fn fixed(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.to_string(),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    }
}

/// `class T { m(a, b, <tail>) { return <tail>.length } }`, plus a module-init
/// `new T().m(1, 2, 3)` so the call site is emitted.
///
/// Three arguments against two declared parameters is the discriminating call:
/// it is the shape where "bundle from 0" and "bundle from declared - 1" differ
/// in how many elements land in the array (3 vs 1).
fn module_with_tail(tail: Param) -> Module {
    let mut m = Module::new("class_method_arguments.ts");
    m.classes = vec![Class {
        id: 401,
        name: "T".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: None,
        methods: vec![Function {
            id: 20,
            name: "m".to_string(),
            type_params: Vec::new(),
            params: vec![fixed(A_ID, "a"), fixed(B_ID, "b"), tail],
            return_type: Type::Any,
            body: vec![Stmt::Return(Some(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(TAIL_ID)),
                property: "length".to_string(),
                byte_offset: 0,
            }))],
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }],
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
    }];
    m.init = vec![
        Stmt::Let {
            id: RECV_ID,
            name: "t".to_string(),
            ty: Type::Named("T".to_string()),
            mutable: false,
            init: Some(Expr::New {
                class_name: "T".to_string(),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        },
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(RECV_ID)),
                property: "m".to_string(),
                byte_offset: 0,
            }),
            args: vec![Expr::Number(1.0), Expr::Number(2.0), Expr::Number(3.0)],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    ];
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn emit(m: &Module) -> String {
    String::from_utf8(compile_module(m, ir_opts()).unwrap()).expect("LLVM IR should be UTF-8")
}

/// Count the `js_array_push_f64` calls that carry each of the three argument
/// literals, in the module-init function where the call site lives.
fn pushes_of(ir: &str, literal: &str) -> usize {
    ir.lines()
        .filter(|l| l.contains(PUSH) && l.contains(literal))
        .count()
}

/// The regression. Three args, two declared params, body reads `arguments`:
/// all three must be pushed into the bundle, and the bundle must be marked.
#[test]
fn a_class_method_reading_arguments_is_handed_every_passed_argument() {
    let ir = emit(&module_with_tail(synthetic_arguments_param()));

    assert!(
        ir.contains(MARK),
        "the class-method call site never marked its synthesized `arguments` \
         array — the callee receives a plain Array, so `arguments` fails every \
         arguments-object predicate:\n{ir}"
    );

    // The discriminating quantity. Bundling from `declared - 1` pushes ONLY the
    // third argument; bundling from 0 pushes all three. Asserting merely that
    // "a push happened" passes in both worlds and is worthless here.
    for (literal, label) in [
        ("double 1.0", "first"),
        ("double 2.0", "second"),
        ("double 3.0", "third"),
    ] {
        assert!(
            pushes_of(&ir, literal) >= 1,
            "the {label} argument ({literal}) was never pushed into the \
             synthesized `arguments` array; the call site bundled from \
             `declared - 1` instead of from argument 0, so \
             `arguments.length` is {n} rather than 3:\n{ir}",
            n = ["double 1.0", "double 2.0", "double 3.0"]
                .iter()
                .filter(|l| pushes_of(&ir, l) >= 1)
                .count(),
        );
    }
}

/// The safety half: a real `...rest` with no `arguments` read keeps the old
/// shape. It bundles only the args PAST the declared params, and is not marked.
///
/// Without this, "bundle everything from 0 and always mark" passes the positive
/// test while silently rewriting `m(a, b, ...rest)` so `rest` aliases the full
/// argument list.
#[test]
fn a_user_rest_parameter_still_bundles_only_its_trailing_arguments() {
    let ir = emit(&module_with_tail(user_rest_param()));

    assert!(
        !ir.contains(MARK),
        "a user `...rest` array was marked as an arguments object; the #8040 \
         fix widened past the parameter it was scoped to:\n{ir}"
    );
    assert_eq!(
        pushes_of(&ir, "double 3.0"),
        1,
        "the trailing argument must still be bundled into the user rest \
         array:\n{ir}"
    );
    assert_eq!(
        pushes_of(&ir, "double 1.0"),
        0,
        "argument 0 is a DECLARED parameter of `m(a, b, ...rest)` and must be \
         passed positionally, never folded into the rest array:\n{ir}"
    );
}
