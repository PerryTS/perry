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
//! For methods which need the materialized object, both halves matter and both
//! are asserted:
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
        namespace_self_import: false,
        static_shape_ids: Vec::new(),
        program_layout_ordinals: Default::default(),
        program_class_shape_ids: Default::default(),
        program_facts: Default::default(),
        program_layouts: Default::default(),
        gc_effects: Default::default(),
        target: None,
        is_entry_module: true,
        non_entry_module_prefixes: Vec::new(),
        thread_literal_module_prefixes: Vec::new(),
        nextjs_path_init_modules: Vec::new(),
        lazy_path_init_modules: Vec::new(),
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
        namespace_source_prefixes: Default::default(),
        namespace_member_nested: Vec::new(),
        cjs_interop_namespaces: Vec::new(),
        constructor_param_counts: Default::default(),
        imported_classes: Vec::new(),
        short_spread_method_candidates: std::sync::Arc::default(),
        program_class_accessor_names: Default::default(),
        object_literal_method_candidates: std::sync::Arc::default(),
        imported_enums: Vec::new(),
        imported_async_funcs: std::collections::HashSet::new(),
        type_aliases: Default::default(),
        imported_func_param_counts: std::collections::HashMap::new(),
        imported_func_has_rest: std::collections::HashSet::new(),
        imported_func_synthetic_arguments: std::collections::HashSet::new(),
        imported_func_return_types: std::collections::HashMap::new(),
        imported_vars: std::collections::HashSet::new(),
        output_type: "executable".to_string(),
        disable_constfn_shapes: false,
        program_has_worker: false,
        program_has_thread_agents: false,
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
        deferred_module_prefixes: Default::default(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        namespace_adopts_cjs_exports: false,
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
                byte_end: 0,
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
                byte_end: 0,
                class_name: "T".to_string(),
                args: Vec::new(),
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        },
        Stmt::Expr(Expr::Call {
            byte_end: 0,
            callee: Box::new(Expr::PropertyGet {
                byte_end: 0,
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

#[test]
fn an_entry_observation_needs_no_readiness_state() {
    let mut module = module_with_tail(synthetic_arguments_param());
    module.classes[0].methods[0].body = vec![Stmt::Return(Some(Expr::LocalGet(TAIL_ID)))];
    let ir = emit(&module);
    let method = ir
        .split("define ")
        .find(|body| {
            body.lines()
                .next()
                .is_some_and(|line| line.contains("__T__m("))
        })
        .expect("the public method must be emitted");
    let method = method.split("\n}").next().unwrap();
    assert!(method.contains("call i64 @js_arguments_object_alloc"));
    assert!(!method.contains("arguments.birth"));
    assert!(!method.contains("arguments.ready"));
}

#[test]
fn an_unexecuted_arguments_branch_does_not_birth_an_object() {
    let mut module = module_with_tail(synthetic_arguments_param());
    module.classes[0].methods[0].body = vec![Stmt::If {
        condition: Expr::Compare {
            op: perry_hir::CompareOp::Lt,
            left: Box::new(Expr::LocalGet(A_ID)),
            right: Box::new(Expr::Number(0.0)),
        },
        then_branch: vec![Stmt::Return(Some(Expr::LocalGet(TAIL_ID)))],
        else_branch: Some(vec![Stmt::Return(Some(Expr::LocalGet(B_ID)))]),
    }];
    let ir = emit(&module);
    let method = ir
        .split("define ")
        .find(|body| {
            body.lines()
                .next()
                .is_some_and(|line| line.contains("__T__m("))
        })
        .expect("the public class method must be emitted");
    let method = method.split("\n}").next().unwrap();
    let allocation = method
        .find("call i64 @js_arguments_object_alloc")
        .expect("the taken branch must still create an arguments object");
    let first_branch = method.find("br i1").expect("the cold branch must be live");
    assert!(
        first_branch < allocation,
        "arguments birth must not dominate the source branch:\n{method}"
    );
    assert!(!method[..first_branch].contains("@js_arguments_object_alloc"));
}

/// Count the `js_array_push_f64` calls that carry each of the three argument
/// literals, in the module-init function where the call site lives.
fn pushes_of(ir: &str, literal: &str) -> usize {
    ir.lines()
        .filter(|l| l.contains(PUSH) && l.contains(literal))
        .count()
}

/// The regression. Three args, two declared params, body reads `arguments[0]`:
/// all three must be pushed into the bundle, and the bundle must be marked.
///
/// The index read deliberately prevents the length-only direct ABI from
/// scalarizing the object. This continues to cover the materialized path which
/// originally lost the first two arguments in #8040.
#[test]
fn a_class_method_reading_arguments_is_handed_every_passed_argument() {
    let mut module = module_with_tail(synthetic_arguments_param());
    module.classes[0].methods[0].body = vec![Stmt::Return(Some(Expr::IndexGet {
        byte_offset: 0,
        byte_end: 0,
        object: Box::new(Expr::LocalGet(TAIL_ID)),
        index: Box::new(Expr::Integer(0)),
    }))];
    let ir = emit(&module);

    assert!(
        ir.contains(MARK),
        "the managed arguments snapshot must keep its identity:\n{ir}"
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
                .filter(|literal| pushes_of(&ir, literal) >= 1)
                .count(),
        );
    }
}

/// A method which observes only `arguments.length` has an additive, guarded
/// direct ABI. It receives the actual count as a scalar while the registered
/// public method retains the ordinary arguments-object ABI for dynamic calls.
#[test]
fn a_length_only_class_method_direct_call_passes_the_scalar_count() {
    let ir = emit(&module_with_tail(synthetic_arguments_param()));

    assert!(
        ir.contains(
            "call double @perry_method_class_method_arguments_ts__T__m$arguments_length(\
             double"
        ) && ir.contains("double 1.0, double 2.0, double 3.0)"),
        "the guarded direct path should call the length-only clone with the \
         three-argument count in its trailing slot:\n{ir}"
    );
    assert!(
        !ir.contains(MARK)
            && !ir.contains("call i64 @js_array_alloc(")
            && !ir.contains("call i64 @js_array_push_f64("),
        "the length-only direct call should not materialize an arguments \
         object:\n{ir}"
    );
    assert!(
        ir.contains("ptr @perry_method_class_method_arguments_ts__T__m, ptr @")
            && !ir.contains(
                "ptr @perry_method_class_method_arguments_ts__T__m$arguments_length, ptr"
            ),
        "the class declaration must continue to publish the public method, \
         never the scalar-only clone:\n{ir}"
    );
}

/// An exact-family proof elides guards, but must retain the scalar clone ABI.
#[test]
fn a_length_only_exact_family_call_keeps_the_scalar_arguments_abi() {
    let module = module_with_tail(synthetic_arguments_param());
    let ir = crate::proto_constants::with_test_census(Default::default(), || emit(&module));
    // Entry thunks legitimately forward arguments objects to the public body;
    // inspect the source call site, where the trailing value is a scalar.
    let ir = ir
        .split("\ndefine ")
        .find(|body| body.starts_with("i32 @main()"))
        .expect("the entry function containing t.m(1, 2, 3)");
    assert!(
        ir.contains("call double @perry_method_class_method_arguments_ts__T__m$arguments_length("),
        "the exact-family call must select the scalar clone:\n{ir}"
    );
    assert!(
        !ir.contains("call double @perry_method_class_method_arguments_ts__T__m("),
        "the public method expects an arguments object, not the scalar count:\n{ir}"
    );
    assert!(
        !ir.contains("method_direct.")
            && !ir.contains(MARK)
            && !ir.contains("call i64 @js_array_alloc("),
        "the exact-family proof must retain its guard-free, allocation-free call:\n{ir}"
    );
}

/// A family's extra late key requires a separate, layout-independent call.
#[test]
fn a_length_only_family_call_keeps_the_scalar_arguments_abi() {
    let mut module = module_with_tail(synthetic_arguments_param());
    // A key introduced by a method belongs to the family, but is not part
    // of the constructor's initialized field layout. The family guard must
    // therefore use its separate call block rather than a layout clone.
    module.classes[0].methods[0].body.insert(
        0,
        Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::This),
            property: "late".into(),
            value: Box::new(Expr::Number(1.0)),
            byte_offset: 0,
            byte_end: 0,
        }),
    );
    let mut opts = ir_opts();
    opts.program_layouts = std::sync::Arc::new(crate::layout_families::compute(
        [(module.name.as_str(), &module)],
        true,
        &Default::default(),
    ));
    let family = opts.program_layouts.families.get(&401).unwrap();
    assert!(family.keys.iter().any(|(key, _)| key == "late"));
    assert_eq!(family.init_count, 0);
    let ir = String::from_utf8(compile_module(&module, opts).unwrap()).unwrap();
    let mut blocks = 0;
    let mut lines = ir.lines();
    while let Some(line) = lines.next() {
        if line.starts_with("method_direct.family_call.") && line.ends_with(':') {
            let body = lines
                .by_ref()
                .take_while(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                body.contains(
                    "call double @perry_method_class_method_arguments_ts__T__m$arguments_length("
                ),
                "the family call must pass its scalar count to the scalar clone:\n{body}\n{ir}"
            );
            assert!(
                !body.contains("call double @perry_method_class_method_arguments_ts__T__m("),
                "the public body expects a real arguments object:\n{body}"
            );
            blocks += 1;
        }
    }
    assert!(
        blocks > 0,
        "fixture must exercise a family call block:\n{ir}"
    );
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
