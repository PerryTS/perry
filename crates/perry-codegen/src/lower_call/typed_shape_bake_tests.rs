//! The class birth ShapeId carries the field representation. Inline object
//! allocation writes one header image with no object layout-state bits, for
//! both pointer-free and pointer-bearing classes.

use crate::{compile_module, AppMetadata, CompileOptions, ImportedClass};
use perry_hir::types::Type;
use perry_hir::{
    BinaryOp, Class, ClassField, CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt,
    UpdateOp,
};

const DECLARE_CALL: &str = "call void @js_gc_declare_typed_shape_layout(";
const CLASS_SHAPE_MINT_CALL: &str = "call i32 @js_object_shape_id_for_class_keys(";
const FORGET_CALL: &str = "call void @js_gc_forget_object_layout(";

/// The packed object header has type, arena flag, and size; object layout
/// state in the reserved halfword is zero regardless of birth rep.
fn object_header_word() -> String {
    const GC_TYPE_OBJECT: u64 = 0x02;
    const GC_FLAG_ARENA: u64 = 0x02;
    let slots = std::cmp::max(2, crate::target_layout::INLINE_SLOT_FLOOR);
    let size =
        8 + crate::target_layout::object_header_size_bytes("aarch64-apple-darwin") + 8 * slots;
    let word = (size << 32) | (GC_FLAG_ARENA << 8) | GC_TYPE_OBJECT;
    format!("insertelement <2 x i64> <i64 {word}, i64 0>,")
}

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
        program_class_accessor_names: Default::default(),
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
        deferred_module_prefixes: std::collections::HashSet::new(),
        module_init_deps: Vec::new(),
        is_dynamic_import_target: false,
        debug_locations: false,
        module_source: None,
        debug_source_line_offset: 0,
    }
}

fn field(name: &str, ty: Type) -> ClassField {
    ClassField {
        name: name.to_string(),
        key_expr: None,
        ty,
        init: None,
        is_private: false,
        is_readonly: false,
        decorators: Vec::new(),
    }
}

const A_ID: u32 = 1;
const B_ID: u32 = 2;
const I_ID: u32 = 7;

/// `constructor(a, b) { this.<f0> = a; this.<f1> = b }` — the maximal
/// param-assigned prologue `ctor_prologue_param_assigned_fields` admits, which
/// is what makes the layout declarable at allocation at all (#7510).
fn two_field_ctor(f0: &str, f1: &str, f1_ty: Type) -> Function {
    Function {
        id: 900,
        name: "constructor".to_string(),
        type_params: Vec::new(),
        params: vec![
            Param {
                id: A_ID,
                name: "a".to_string(),
                ty: Type::Number,
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
            Param {
                id: B_ID,
                name: "b".to_string(),
                ty: f1_ty,
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            },
        ],
        return_type: Type::Void,
        body: vec![
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: f0.to_string(),
                value: Box::new(Expr::LocalGet(A_ID)),
            }),
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: f1.to_string(),
                value: Box::new(Expr::LocalGet(B_ID)),
            }),
        ],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn two_field_class(name: &str, f1_ty: Type) -> Class {
    Class {
        id: 404,
        name: name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![field("a", Type::Number), field("b", f1_ty.clone())],
        constructor: Some(two_field_ctor("a", "b", f1_ty)),
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
    }
}

/// `for (let i = 0; i < 1000; i++) { const x = new <name>(i, <second>); }`
///
/// The loop is load-bearing: `new_site_is_in_loop` is what selects the inline
/// bump allocator, and only the inline bump has a packed header constant to
/// fold the layout into. An outlined `js_object_alloc_class_inline_keys` site
/// keeps the runtime declare, by design.
pub(super) fn loop_new_module(name: &str, f1_ty: Type, second: Expr) -> Module {
    let mut m = Module::new("typed_shape_bake.ts");
    m.classes = vec![two_field_class(name, f1_ty)];
    m.init = vec![Stmt::For {
        init: Some(Box::new(Stmt::Let {
            id: I_ID,
            name: "i".to_string(),
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Integer(0)),
        })),
        condition: Some(Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(Expr::LocalGet(I_ID)),
            right: Box::new(Expr::Integer(1000)),
        }),
        update: Some(Expr::Update {
            id: I_ID,
            op: UpdateOp::Increment,
            prefix: false,
        }),
        body: vec![
            Stmt::Let {
                id: 20,
                name: "x".to_string(),
                ty: Type::Named(name.to_string()),
                mutable: false,
                init: Some(Expr::New {
                    class_name: name.to_string(),
                    args: vec![
                        Expr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(Expr::LocalGet(I_ID)),
                            right: Box::new(Expr::Integer(1)),
                        },
                        second,
                    ],
                    type_args: Vec::new(),
                    byte_offset: 0,
                    cap_args_appended: 0,
                }),
            },
            // ESCAPE. Without this the instance never leaves the iteration and
            // is scalar-replaced away — the test would then assert about an
            // allocation the compiler deleted, and would pass on a build where
            // the bake does nothing. (`collectors/escape_news.rs`; the same trap
            // is called out in the campaign's measurement protocol.)
            Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::FuncRef(10)),
                args: vec![Expr::LocalGet(20)],
                type_args: Vec::new(),
                byte_offset: 0,
            }),
        ],
    }];
    m.functions = vec![Function {
        id: 10,
        name: "sink".to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            id: 30,
            name: "p".to_string(),
            ty: Type::Named(name.to_string()),
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(30)),
            property: "a".to_string(),
            byte_offset: 0,
        }))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: true,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }];
    m.init_kind = ModuleInitKind::Eager;
    m
}

/// One escaping `new <name>(1, <second>)` outside a loop. This selects the
/// stamped outlined allocator and covers specialized/cold entries that do not
/// inherit a caller's loop or allocation-hot classification.
pub(super) fn outlined_new_module(name: &str, f1_ty: Type, second: Expr) -> Module {
    let mut module = loop_new_module(name, f1_ty, second.clone());
    module.init = vec![
        Stmt::Let {
            id: 20,
            name: "x".to_string(),
            ty: Type::Named(name.to_string()),
            mutable: false,
            init: Some(Expr::New {
                class_name: name.to_string(),
                args: vec![Expr::Integer(1), second],
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        },
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(10)),
            args: vec![Expr::LocalGet(20)],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    ];
    module
}

pub(super) fn emit(m: &Module) -> String {
    String::from_utf8(compile_module(m, ir_opts()).unwrap()).expect("LLVM IR should be UTF-8")
}

/// A numeric birth carries its rep in the ShapeId and leaves header
/// layout-state bits clear.
#[test]
fn a_pointer_free_birth_uses_the_shape_rep_and_no_object_layout_state() {
    let ir = emit(&loop_new_module("Pair", Type::Number, Expr::Integer(2)));
    assert!(
        ir.contains(&object_header_word()),
        "missing object header image:
{ir}"
    );
    assert!(
        !ir.contains(DECLARE_CALL),
        "per-instance typed layout declaration survived:
{ir}"
    );
    assert!(
        !ir.contains(FORGET_CALL),
        "object address-keyed layout cleanup survived:
{ir}"
    );
}

/// A pointer-bearing birth uses the same header image; its ShapeId carries
/// the different rep and the collector traces that rep exactly.
#[test]
fn a_pointer_bearing_birth_uses_the_same_header_image_and_its_own_shape() {
    let ir = emit(&loop_new_module(
        "Link",
        Type::Union(vec![Type::Named("Link".to_string()), Type::Null]),
        Expr::Null,
    ));
    assert!(
        ir.contains(CLASS_SHAPE_MINT_CALL),
        "birth ShapeId mint absent:
{ir}"
    );
    assert!(
        ir.contains(&object_header_word()),
        "birth header differs by rep:
{ir}"
    );
    assert!(
        !ir.contains(DECLARE_CALL),
        "per-instance typed layout declaration survived:
{ir}"
    );
    assert!(
        !ir.contains(FORGET_CALL),
        "object address-keyed layout cleanup survived:
{ir}"
    );
}

/// The return-override's `undefined` arm, decided inline.
///
/// Asserted together with the surviving call: the change is "answer the common
/// case with one compare", never "stop applying the spec rule". A constructor
/// that returns an object, an arguments object or an array — and a derived
/// constructor that returns a primitive, which must throw — all still route to
/// the runtime.
#[test]
fn an_undefined_constructor_completion_takes_the_inline_arm() {
    let ir = emit(&loop_new_module("Pair", Type::Number, Expr::Integer(2)));
    assert!(
        ir.contains("ctor_ret.merge"),
        "the inline `undefined` arm was not emitted, so every construction \
         still calls `js_ctor_return_override` (8% of `churn_alloc`):\n{ir}"
    );
    assert!(
        ir.contains(", 9222246136947933185") && ir.contains("ctor_ret.override"),
        "the inline arm must be exactly the TAG_UNDEFINED bit compare — \
         `JSValue::is_undefined` is `bits == TAG_UNDEFINED`, which is what \
         makes returning `this` here equal to what the runtime returns:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_ctor_return_override("),
        "the runtime call must survive on the cold arm; without it a \
         constructor returning an object would be ignored and a derived one \
         returning a primitive would not throw:\n{ir}"
    );
}

fn imported_remote() -> ImportedClass {
    ImportedClass {
        name: "Remote".to_string(),
        local_alias: None,
        namespace: None,
        source_prefix: "producer_ts".to_string(),
        constructor_param_count: 1,
        has_own_constructor: true,
        constructor_has_rest: false,
        constructor_has_synthetic_arguments: false,
        has_instance_fields: true,
        method_names: vec!["read".to_string()],
        proven_this_method_names: Vec::new(),
        proven_this_tower_method_names: Vec::new(),
        method_return_types: vec![Type::Number],
        method_param_counts: vec![0],
        method_has_rest: vec![false],
        method_has_synthetic_arguments: vec![false],
        method_arguments_length_only: vec![false],
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
        field_names: vec!["child".to_string()],
        field_types: vec![Type::Union(vec![
            Type::Named("Remote".to_string()),
            Type::Null,
        ])],
        source_class_id: Some(55),
        return_shape_imports: Vec::new(),
        object_literal: None,
    }
}

/// Import metadata may retain a name that a local class shadows. Such a class
/// still has its own constructor proof and must not be mistaken for the
/// body-less imported stub when choosing the at-allocation layout.
#[test]
fn a_local_class_shadowing_an_import_keeps_its_layout_proof() {
    let module = loop_new_module(
        "Remote",
        Type::Union(vec![Type::Named("Remote".to_string()), Type::Null]),
        Expr::Null,
    );
    let mut opts = ir_opts();
    opts.imported_classes.push(imported_remote());

    let ir =
        String::from_utf8(compile_module(&module, opts).unwrap()).expect("LLVM IR should be UTF-8");
    assert!(
        ir.contains(CLASS_SHAPE_MINT_CALL) && !ir.contains(DECLARE_CALL),
        "the local constructor proof was suppressed by a shadowed import:\n{ir}"
    );
}

/// A consumer knows an imported class's declared field types, but its HIR stub
/// deliberately has no constructor body. That absence is not proof that the
/// fields may be declared before the real cross-module constructor runs.
///
/// More subtly, letting the consumer infer the declaration mints a dedicated
/// typed ShapeId here while the defining module may have minted the ordinary
/// structural id. Exact method guards compiled in the producer then reject
/// every instance allocated in this module despite the class id and keys being
/// identical.
#[test]
fn imported_pointer_layout_does_not_invent_a_consumer_typed_shape_id() {
    let mut module = Module::new("imported_shape_consumer.ts");
    module.init = vec![Stmt::Let {
        id: 20,
        name: "instance".to_string(),
        ty: Type::Named("Remote".to_string()),
        mutable: false,
        init: Some(Expr::New {
            class_name: "Remote".to_string(),
            args: vec![Expr::Null],
            type_args: Vec::new(),
            byte_offset: 0,
            cap_args_appended: 0,
        }),
    }];

    let mut opts = ir_opts();
    opts.imported_classes.push(imported_remote());

    let ir =
        String::from_utf8(compile_module(&module, opts).unwrap()).expect("LLVM IR should be UTF-8");
    assert!(
        ir.contains("call i32 @js_object_shape_id_for_class_keys("),
        "the consumer must share the producer's canonical structural ShapeId:\n{ir}"
    );
    assert!(
        !ir.contains(DECLARE_CALL) && !ir.contains("call void @js_gc_init_typed_shape_layout("),
        "charter step 5: no per-instance layout install for an imported class:\n{ir}"
    );
}

#[test]
fn imported_length_only_arguments_capability_uses_scalar_direct_abi() {
    let mut module = Module::new("imported_arguments_length_consumer.ts");
    module.init = vec![
        Stmt::Let {
            id: 20,
            name: "instance".to_string(),
            ty: Type::Named("Remote".to_string()),
            mutable: false,
            init: Some(Expr::New {
                class_name: "Remote".to_string(),
                args: vec![Expr::Null],
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        },
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(20)),
                property: "read".to_string(),
                byte_offset: 0,
            }),
            args: vec![Expr::Integer(1), Expr::Integer(2)],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    ];

    let mut remote = imported_remote();
    remote.method_param_counts = vec![1];
    remote.method_has_rest = vec![true];
    remote.method_has_synthetic_arguments = vec![true];
    remote.method_arguments_length_only = vec![true];
    let mut opts = ir_opts();
    opts.imported_classes.push(remote);

    let ir =
        String::from_utf8(compile_module(&module, opts).unwrap()).expect("LLVM IR should be UTF-8");
    assert!(
        ir.contains("declare double @perry_method_producer_ts__Remote__read$arguments_length")
            && ir.contains("call double @perry_method_producer_ts__Remote__read$arguments_length",)
            && ir.contains("double 2.0"),
        "the consumer should trust the producer capability and pass only the actual count:\n{ir}"
    );
    assert!(
        !ir.contains("call i64 @js_array_alloc")
            && !ir.contains("call i64 @js_array_push_f64")
            && !ir.contains("call i64 @js_array_mark_arguments_object"),
        "the imported direct path should not allocate an argument bundle:\n{ir}"
    );
}

/// A module's string pool can run before the defining module of a class it
/// imports has initialized (the entry module, an import cycle). With link-time
/// ids (design step 4) the stub's mint carries the static id the driver gave
/// its content, so either init order converges on one id without the runtime
/// keeping any module's global addresses. The birth rep is content (charter
/// step 5, T1): an importer's all-`Any` stub requests the definer's id only
/// when the definer is all-`Any` too; a definer born with an `F64` lane is
/// another content, and the stub keeps its own id.
#[test]
fn imported_stub_mints_with_the_drivers_static_id_and_registers_no_slots() {
    use crate::{BirthShape, DefinedClassShape, ProgramClassShapeIds};
    let module = || {
        let mut module = Module::new("imported_shape_slots.ts");
        module.init = vec![Stmt::Let {
            id: 20,
            name: "instance".to_string(),
            ty: Type::Named("Remote".to_string()),
            mutable: false,
            init: Some(Expr::New {
                class_name: "Remote".to_string(),
                args: vec![Expr::Null],
                type_args: Vec::new(),
                byte_offset: 0,
                cap_args_appended: 0,
            }),
        }];
        module
    };
    let mut opts = ir_opts();
    opts.imported_classes.push(imported_remote());
    let births = crate::module_birth_shapes(&module(), opts.clone()).unwrap();
    assert_eq!(births.len(), 1, "the stub is this module's one class birth");
    assert!(!births[0].defined, "an imported stub is not a definition");
    let stub = births[0].shape.clone();
    assert_eq!(stub.rep, 0, "an imported stub is born all-Any");

    // `definer_rep`: the defining module's birth rep for the same keys. Returns
    // the id the stub's mint requests, after checking the mint's shape.
    let mint_id = |definer_rep: u64| -> (u32, u32, u32) {
        let definer = BirthShape {
            rep: definer_rep,
            ..stub.clone()
        };
        let ids = crate::assign_static_shape_ids([&stub, &definer]);
        let (own, def_id) = (ids[&stub], ids[&definer]);
        let mut opts = opts.clone();
        opts.static_shape_ids = vec![(stub.clone(), own)];
        opts.program_class_shape_ids = ProgramClassShapeIds(
            [(
                55,
                DefinedClassShape {
                    keys_global: "perry_class_keys_producer_ts__Remote".to_string(),
                    shape: definer,
                    id: def_id,
                },
            )]
            .into_iter()
            .collect(),
        );
        let ir = String::from_utf8(compile_module(&module(), opts).unwrap())
            .expect("LLVM IR should be UTF-8");
        let mint = ir
            .lines()
            .find(|l| l.contains("call i32 @js_object_shape_id_for_class_keys_static("))
            .unwrap_or_else(|| panic!("the stub must mint with its static id:\n{ir}"));
        assert!(
            mint.contains("i32 55,") && mint.ends_with(", i64 0)"),
            "the mint must carry the class id and the stub's all-Any rep:\n{mint}"
        );
        assert!(
            !ir.contains("js_register_imported_class_shape_slot"),
            "no module global address is handed to the runtime any more:\n{ir}"
        );
        // S6: there is no poisonable guard twin to seed or register any more;
        // the class-field guards compare against the ShapeId global itself.
        assert!(
            !ir.contains("perry_class_guard_shape_"),
            "no poisonable guard expectation may be emitted:\n{ir}"
        );
        let requested = [own, def_id]
            .into_iter()
            .find(|id| mint.contains(&format!("i32 {id}, i64 0)")))
            .unwrap_or_else(|| panic!("the mint requests neither {own} nor {def_id}:\n{mint}"));
        (requested, own, def_id)
    };

    // Both all-Any: one content, so the stub requests the definer's id.
    let (requested, own, def_id) = mint_id(0);
    assert_eq!(own, def_id, "equal contents get one id");
    assert_eq!(
        requested, def_id,
        "the all-Any stub must adopt the definer's id"
    );

    // The definer has an F64 lane (slot 0): two contents, two ids, and the
    // stub keeps its own.
    let (requested, own, def_id) = mint_id(0b01);
    assert_ne!(
        own, def_id,
        "an F64 birth rep is content: one id would name two layouts"
    );
    assert_eq!(
        requested, own,
        "an all-Any stub must never adopt an F64 definer's id"
    );
}
