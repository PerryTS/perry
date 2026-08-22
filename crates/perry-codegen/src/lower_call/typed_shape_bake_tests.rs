//! #7834: the at-allocation typed-shape layout, folded into the header
//! constant — and the `undefined` return-override decided inline.
//!
//! Both are IR-census tests, and both are the "assert the subject was live"
//! kind (CLAUDE.md). An optimisation whose predicate quietly answers `false`
//! everywhere still compiles, still prints the right answer, and shows up in no
//! other test — `js_gc_declare_typed_shape_layout` was 30% of `churn_alloc` and
//! nothing but a profile said so.
//!
//! ## What the positive asserts
//!
//! For a class whose pointer mask is statically EMPTY, the canonical layout is
//! the constant `GC_LAYOUT_POINTER_FREE | GC_OBJ_TYPED_LAYOUT_INTACT`, so the
//! inline-bump path stamps it into the packed `GcHeader` store it was already
//! emitting and drops the per-instance call. What survives is the one half that
//! depends on the recycled ADDRESS rather than on the shape — clearing a
//! previous tenant's per-object record — behind a `PERRY_PER_OBJECT_LAYOUTS_ANY`
//! test whose `0` state proves every thread's tables empty.
//!
//! ## Pointer-bearing layouts
//!
//! #8405 registers their immutable mask once at module init under a dedicated
//! typed ShapeId. That makes `GC_LAYOUT_SIDE_MASK | GC_OBJ_TYPED_LAYOUT_INTACT`
//! complete before the first object is allocated, so this case now drops the
//! per-instance declare too. The test asserts both halves: the one-time typed
//! ShapeId call and the baked header state.
//!
//! ## Why the pointer-free bake needs no descriptor
//!
//! `heap_payload_slot_selection` skips a `GC_LAYOUT_POINTER_FREE` payload
//! outright, without consulting any map — so the collector's view is
//! bit-identical to the pre-#7834 one, which also reached `POINTER_FREE` for an
//! empty pointer mask. And a later pointer store still downgrades: with no
//! descriptor to classify against, `layout_note_slot` falls through to its
//! generic pointer-mask branch, which mints a per-object mask and flips the
//! state to `SIDE_MASK`. That branch needs no descriptor at all.

use crate::{compile_module, AppMetadata, CompileOptions, ImportedClass};
use perry_hir::types::Type;
use perry_hir::{
    BinaryOp, Class, ClassField, CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt,
    UpdateOp,
};

/// The six-argument per-instance declare this ticket removes.
const DECLARE_CALL: &str = "call void @js_gc_declare_typed_shape_layout(";
const TYPED_SHAPE_MINT_CALL: &str = "call i32 @js_gc_typed_shape_id_for_keys(";
/// The one-argument address-only remainder that replaces it.
const FORGET_CALL: &str = "call void @js_gc_forget_object_layout(";
/// The process-global emptiness proof the remainder is gated on.
const ANY_GLOBAL: &str = "@PERRY_PER_OBJECT_LAYOUTS_ANY";
const ANY_ATOMIC_LOAD: &str =
    "load atomic i32, ptr @PERRY_PER_OBJECT_LAYOUTS_ANY monotonic, align 4";

/// The packed `GcHeader` word the inline bump writes for a two-`number`-field
/// class:
///
/// ```text
///   obj_type  GC_TYPE_OBJECT                     = 0x02   bits  0..7
///   gc_flags  GC_FLAG_ARENA                      = 0x02   bits  8..15
///   _reserved GC_LAYOUT_POINTER_FREE [| INTACT]  = 0x4000 [| 0x1000]  bits 16..31
///   size      8 + 32 + max(2, INLINE_SLOT_FLOOR)*8       bits 32..63
/// ```
///
/// Computed from `INLINE_SLOT_FLOOR` rather than spelled as a literal: #7916
/// moved the floor 4 → 2, which changes `size` 72 → 56 and therefore both
/// words. A hard-coded constant here fails the moment the footprint changes
/// and says nothing about what this test is actually for (whether
/// `GC_OBJ_TYPED_LAYOUT_INTACT` is claimed), so derive the part that is
/// incidental and keep asserting the part that is not.
fn header_word(layout_state: u64, intact: bool) -> String {
    const GC_TYPE_OBJECT: u64 = 0x02;
    const GC_FLAG_ARENA: u64 = 0x02;
    const GC_OBJ_TYPED_LAYOUT_INTACT: u64 = 0x1000;
    let slots = std::cmp::max(2, crate::target_layout::INLINE_SLOT_FLOOR);
    let size =
        8 + crate::target_layout::object_header_size_bytes("aarch64-apple-darwin") + 8 * slots;
    let reserved = layout_state
        | if intact {
            GC_OBJ_TYPED_LAYOUT_INTACT
        } else {
            0
        };
    let word = (size << 32) | (reserved << 16) | (GC_FLAG_ARENA << 8) | GC_TYPE_OBJECT;
    // #8122: the packed word is no longer a per-site scalar store — it is the
    // constant lane of the per-class `<2 x i64>` header image composed once at
    // module init (`insertelement <2 x i64> <i64 WORD, i64 0>, i64 %shape_word,
    // i32 1`), which every inline `new` of the class stores as one vector.
    format!("insertelement <2 x i64> <i64 {word}, i64 0>,")
}

/// The packed word WITH the baked `GC_OBJ_TYPED_LAYOUT_INTACT`.
fn baked_header_word() -> String {
    header_word(0x4000, true)
}
/// The same word WITHOUT it — what the pointer-bearing class still writes.
fn unbaked_header_word() -> String {
    header_word(0x4000, false)
}
/// A registered pointer-bearing class starts in SIDE_MASK with an intact
/// descriptor reachable through its dedicated typed ShapeId.
fn side_mask_baked_header_word() -> String {
    header_word(0x8000, true)
}

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

/// `class Pair { a: number; b: number }` — pointer mask statically empty.
#[test]
fn a_pointer_free_shape_bakes_its_layout_into_the_header_constant() {
    let ir = emit(&loop_new_module("Pair", Type::Number, Expr::Integer(2)));
    assert!(
        ir.contains(&baked_header_word()),
        "the inline-bump header constant does not carry \
         GC_OBJ_TYPED_LAYOUT_INTACT, so the bake did not fire and every \
         construction still pays the runtime declare:\n{ir}"
    );
    assert!(
        !ir.contains(DECLARE_CALL),
        "the per-instance `js_gc_declare_typed_shape_layout` is still emitted \
         for a pointer-free shape — this is the 30% of `churn_alloc` the \
         ticket removes:\n{ir}"
    );
    assert!(
        ir.contains(FORGET_CALL) && ir.contains(ANY_GLOBAL) && ir.contains(ANY_ATOMIC_LOAD),
        "the address-dependent half must survive, gated on the global \
         emptiness proof: a recycled address can carry a previous tenant's \
         per-object mask, and `layout_note_slot` would then OR the new \
         object's pointer bits into it:\n{ir}"
    );
}

/// `class Link { a: number; b: Link | null }` — one declared type differs;
/// everything else about the program is identical.
#[test]
fn a_pointer_bearing_shape_registers_once_and_bakes_the_side_mask() {
    let ir = emit(&loop_new_module(
        "Link",
        Type::Union(vec![Type::Named("Link".to_string()), Type::Null]),
        Expr::Null,
    ));
    assert!(
        !ir.contains(DECLARE_CALL),
        "the per-instance declare survived for a pointer-bearing shape:\n{ir}"
    );
    assert!(
        ir.contains(TYPED_SHAPE_MINT_CALL),
        "the pointer mask was not registered at module init:\n{ir}"
    );
    assert!(
        ir.contains(&side_mask_baked_header_word())
            && !ir.contains(&unbaked_header_word())
            && !ir.contains(&baked_header_word()),
        "the header image does not carry SIDE_MASK | TYPED_LAYOUT_INTACT:\n{ir}"
    );
    assert!(
        ir.contains(FORGET_CALL) && ir.contains(ANY_ATOMIC_LOAD),
        "the address-dependent stale-record cleanup must survive behind its \
         global emptiness gate:\n{ir}"
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
        source_prefix: "producer_ts".to_string(),
        constructor_param_count: 1,
        has_own_constructor: true,
        constructor_has_rest: false,
        has_instance_fields: true,
        method_names: vec!["read".to_string()],
        method_param_counts: vec![0],
        method_has_rest: vec![false],
        method_has_synthetic_arguments: vec![false],
        static_field_names: Vec::new(),
        static_method_names: Vec::new(),
        getter_names: Vec::new(),
        setter_names: Vec::new(),
        parent_name: None,
        field_names: vec!["child".to_string()],
        field_types: vec![Type::Union(vec![
            Type::Named("Remote".to_string()),
            Type::Null,
        ])],
        source_class_id: Some(55),
        return_shape_imports: Vec::new(),
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
        ir.contains(TYPED_SHAPE_MINT_CALL) && !ir.contains(DECLARE_CALL),
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
        ir.contains("call i32 @js_object_shape_id_for_keys("),
        "the consumer must share the producer's canonical structural ShapeId:\n{ir}"
    );
    assert!(
        !ir.contains(TYPED_SHAPE_MINT_CALL),
        "an imported stub invented a consumer-local typed ShapeId:\n{ir}"
    );
    assert!(
        !ir.contains(DECLARE_CALL) && ir.contains("call void @js_gc_init_typed_shape_layout("),
        "the imported layout must be validated after its real constructor, not declared before it:\n{ir}"
    );
}
