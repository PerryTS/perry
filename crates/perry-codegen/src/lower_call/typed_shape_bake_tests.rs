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
//! ## What the negative asserts
//!
//! A pointer-BEARING shape keeps the full runtime declare. It has to: its state
//! is `GC_LAYOUT_SIDE_MASK`, which means the collector consults a mask, and the
//! shared `SHAPE_LAYOUTS` descriptor that mask lives in is installed by exactly
//! that call. Baking `SIDE_MASK` into the header without it would hand the
//! tracer a masked object with no mask.
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

use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{
    BinaryOp, Class, ClassField, CompareOp, Expr, Function, Module, ModuleInitKind, Param, Stmt,
    UpdateOp,
};

/// The six-argument per-instance declare this ticket removes.
const DECLARE_CALL: &str = "call void @js_gc_declare_typed_shape_layout(";
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
fn header_word(intact: bool) -> String {
    const GC_TYPE_OBJECT: u64 = 0x02;
    const GC_FLAG_ARENA: u64 = 0x02;
    const GC_LAYOUT_POINTER_FREE: u64 = 0x4000;
    const GC_OBJ_TYPED_LAYOUT_INTACT: u64 = 0x1000;
    let slots = std::cmp::max(2, crate::target_layout::INLINE_SLOT_FLOOR);
    let size =
        8 + crate::target_layout::object_header_size_bytes("aarch64-apple-darwin") + 8 * slots;
    let reserved = GC_LAYOUT_POINTER_FREE
        | if intact {
            GC_OBJ_TYPED_LAYOUT_INTACT
        } else {
            0
        };
    let word = (size << 32) | (reserved << 16) | (GC_FLAG_ARENA << 8) | GC_TYPE_OBJECT;
    format!("store i64 {word},")
}

/// The packed word WITH the baked `GC_OBJ_TYPED_LAYOUT_INTACT`.
fn baked_header_word() -> String {
    header_word(true)
}
/// The same word WITHOUT it — what the pointer-bearing class still writes.
fn unbaked_header_word() -> String {
    header_word(false)
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

/// `class Link { a: number; b: Link | null }` — the control. One declared type
/// differs; everything else about the program is identical.
#[test]
fn a_pointer_bearing_shape_keeps_the_runtime_declare() {
    let ir = emit(&loop_new_module(
        "Link",
        Type::Union(vec![Type::Named("Link".to_string()), Type::Null]),
        Expr::Null,
    ));
    assert!(
        ir.contains(DECLARE_CALL),
        "a SIDE_MASK shape MUST keep the declare — it is the only thing that \
         installs the shared `SHAPE_LAYOUTS` descriptor the tracer's mask \
         lookup reads:\n{ir}"
    );
    assert!(
        ir.contains(&unbaked_header_word()) && !ir.contains(&baked_header_word()),
        "the header constant must NOT claim GC_OBJ_TYPED_LAYOUT_INTACT for a \
         shape whose descriptor is installed at runtime:\n{ir}"
    );
    assert!(
        !ir.contains(FORGET_CALL),
        "the standalone forget is only for the baked path; the declare already \
         performs it:\n{ir}"
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
