//! #9417: the instance-method dispatch tower's RECEIVER is a rooted temporary,
//! not an SSA register.
//!
//! `lower_call/property_get/dynamic_dispatch.rs` lowers `object` first — JS
//! evaluation order requires the MemberExpression to be evaluated before the
//! arguments — and consumes it last, in `js_object_get_own_field_or_undef`, the
//! class-id tower and `js_native_call_method`. Between those two points sit the
//! argument expressions, which are arbitrary user code that can allocate. A bare
//! SSA register is not a GC root, so an evacuating young-gen minor in an
//! argument leaves the receiver naming from-space
//! (`docs/src/internals/gc-rooting-invariant.md`, case 3).
//!
//! Nothing faults at the move: the probe fails its `obj_type == GC_TYPE_OBJECT`
//! check on the recycled cell and answers TAG_UNDEFINED, so the wrong answer
//! surfaces several steps downstream. `test-files/test_gap_9417_dispatch_
//! receiver_roots.ts` is the end-to-end half of this, and it fails
//! deterministically without GC environment knobs.
//!
//! # Non-vacuity
//!
//! The positive assertion names the VALUE — the register `makeTagged` produced
//! went into a rooted slot, and the probe read its operand back OUT of that slot
//! — so a compiler that roots nothing cannot satisfy it by emitting nothing. The
//! receiver is deliberately a CALL RESULT rather than a local read: a load out of
//! a shadow slot is a re-readable location that `root_reload` already re-derives,
//! so a `LocalGet` receiver would pass against the unfixed compiler.
//!
//! #9480 narrows that sound window per operand. The tests below pin the receiver
//! across later allocating arguments, each argument across only its allocating
//! suffix, the equivalent known-class tower, and the post-argument rest-bundle
//! safety case. Numeric and final-argument controls fail a lowering that roots
//! every operand unconditionally.

use super::{allocating, entry_opts, under_both_lowerings};
use crate::testing::temp_slots::{
    assert_rooted_across, first_call_result, temp_root_slot_holding, temp_root_slots,
};
use crate::{compile_module, AppMetadata, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Param, Stmt};

const MAKE_FN: u32 = 900;
const TAGGED_CLASS: u32 = 901;
const JOIN_FN: u32 = 902;
const RECV_LOCAL: u32 = 903;
const REST_LOCAL: u32 = 904;
const TYPED_RECV_LOCAL: u32 = 905;

fn empty_fn(id: u32, name: &str, return_type: Type) -> Function {
    Function {
        id,
        name: name.to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type,
        body: vec![Stmt::Return(Some(Expr::Undefined))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

/// A class declaring `join`, so `property` has an implementor and the call takes
/// the dispatch tower instead of a direct static call.
fn tagged_class() -> perry_hir::Class {
    perry_hir::Class {
        id: TAGGED_CLASS,
        name: "Tagged".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![perry_hir::ClassField {
            name: "tag".to_string(),
            key_expr: None,
            ty: Type::String,
            init: None,
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        }],
        constructor: None,
        methods: vec![empty_fn(JOIN_FN, "join", Type::Any)],
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        computed_members: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
        aliases: Vec::new(),
    }
}

fn tagged_class_with_rest_method() -> perry_hir::Class {
    let mut class = tagged_class();
    class.methods[0].params.push(Param {
        id: REST_LOCAL,
        name: "values".to_string(),
        ty: Type::Array(Box::new(Type::Any)),
        default: None,
        decorators: Vec::new(),
        is_rest: true,
        arguments_object: None,
    });
    class
}

/// `const r: any = makeTagged(); r.join(<arg>)` — but with the receiver INLINE
/// in the call, so it is the call's result register and not a slot load.
///
/// `makeTagged` returns `any`, so `receiver_class_name` cannot name a class and
/// `needs_dynamic_dispatch` selects the tower.
fn main_ir_for_dispatch(name: &str, args: Vec<Expr>) -> String {
    main_ir_for_dispatch_case(name, args, Type::Any, false)
}

fn main_ir_for_dispatch_case(
    name: &str,
    args: Vec<Expr>,
    receiver_type: Type,
    method_has_rest: bool,
) -> String {
    let mut module = Module::new(name);
    module.classes.push(if method_has_rest {
        tagged_class_with_rest_method()
    } else {
        tagged_class()
    });
    module
        .functions
        .push(empty_fn(MAKE_FN, "makeTagged", receiver_type));
    // A named local so the module is not optimized down to nothing; the call's
    // receiver is still the inline `makeTagged()` result.
    module.init.push(Stmt::Let {
        id: RECV_LOCAL,
        name: "out".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::Call {
                    callee: Box::new(Expr::FuncRef(MAKE_FN)),
                    args: Vec::new(),
                    type_args: Vec::new(),
                    byte_offset: 0,
                }),
                property: "join".to_string(),
                byte_offset: 0,
            }),
            args,
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    });
    module.init.push(Stmt::Expr(Expr::LocalGet(RECV_LOCAL)));

    let opts = CompileOptions {
        app_metadata: AppMetadata::default(),
        ..entry_opts()
    };
    let bytes =
        compile_module(&module, opts).unwrap_or_else(|e| panic!("codegen failed for {name}: {e}"));
    let ir = String::from_utf8(bytes).expect("LLVM IR should be UTF-8");
    crate::testing::root_slots::function_slice(&ir, "main").to_string()
}

/// A declared `Tagged` local selects the known-class direct/virtual tower.
fn main_ir_for_known_dispatch(name: &str, arg: Expr) -> String {
    let mut module = Module::new(name);
    module.classes.push(tagged_class());
    module
        .functions
        .push(empty_fn(MAKE_FN, "makeTagged", Type::Any));
    module.init.push(Stmt::Let {
        id: TYPED_RECV_LOCAL,
        name: "tagged".to_string(),
        ty: Type::Named("Tagged".to_string()),
        mutable: false,
        init: Some(Expr::Call {
            callee: Box::new(Expr::FuncRef(MAKE_FN)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    });
    module.init.push(Stmt::Let {
        id: RECV_LOCAL,
        name: "out".to_string(),
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::Call {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(TYPED_RECV_LOCAL)),
                property: "join".to_string(),
                byte_offset: 0,
            }),
            args: vec![arg],
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    });
    module.init.push(Stmt::Expr(Expr::LocalGet(RECV_LOCAL)));

    let bytes = compile_module(
        &module,
        CompileOptions {
            app_metadata: AppMetadata::default(),
            ..entry_opts()
        },
    )
    .unwrap_or_else(|e| panic!("codegen failed for {name}: {e}"));
    let ir = String::from_utf8(bytes).expect("LLVM IR should be UTF-8");
    crate::testing::root_slots::function_slice(&ir, "main").to_string()
}

/// THE GAP (#9417). The receiver is produced before the argument and consumed
/// after it, so it must live in a rooted slot and the probe must read it back
/// out of that slot.
///
/// Sabotage: reverting `dynamic_dispatch.rs` to `lower_expr(ctx, object)` +
/// per-argument `lower_expr` fails this assertion with "is never stored into a
/// rooted slot — it lives its whole life in an SSA register".
#[test]
fn a_dispatch_receiver_is_rooted_across_its_argument_list() {
    under_both_lowerings(|lowering| {
        let ir = main_ir_for_dispatch("dispatch_receiver_rooted.ts", vec![allocating()]);
        let producer = first_call_result(&ir, "perry_fn_dispatch_receiver_rooted_ts__makeTagged")
            .unwrap_or_else(|| {
                panic!(
                    "{lowering}: no call to `makeTagged` in `main` — this test has no \
                     subject:\n{ir}"
                )
            });
        assert_rooted_across(
            &ir,
            &producer,
            "js_object_get_own_field_or_undef",
            &format!(
                "{lowering}: #9417 — the dispatch receiver is live across the argument \
                 list, which allocates"
            ),
        );
        let last_arg = first_call_result(&ir, "js_object_alloc").unwrap_or_else(|| {
            panic!("{lowering}: allocating argument emitted no js_object_alloc:\n{ir}")
        });
        assert!(
            temp_root_slot_holding(&ir, &last_arg).is_none(),
            "{lowering}: #9480 — the final argument is consumed without an intervening \
             collection point and must not pay a root slot; {last_arg} was rooted:\n{ir}"
        );
    });
}

/// The control on the other side: with a non-collecting numeric argument, the
/// receiver also has no collection window and the whole dispatch needs zero
/// temp slots. The heap fixture needs exactly one: the receiver across that
/// argument's allocation, never the final argument itself.
///
/// Without this half, a lowering that roots every operand unconditionally would
/// pass the test above and pay for it on every call in the program.
#[test]
fn a_numeric_dispatch_argument_still_pays_no_temp_slot() {
    under_both_lowerings(|lowering| {
        let heap = main_ir_for_dispatch("dispatch_receiver_heap_arg.ts", vec![allocating()]);
        let numeric =
            main_ir_for_dispatch("dispatch_receiver_numeric_arg.ts", vec![Expr::Integer(7)]);
        let heap_slots = temp_root_slots(&heap).len();
        let numeric_slots = temp_root_slots(&numeric).len();
        assert_eq!(
            heap_slots,
            numeric_slots + 1,
            "{lowering}: the heap fixture adds only the receiver root; unrelated \
             dispatch bookkeeping is shared (heap {heap_slots}, numeric {numeric_slots})"
        );
    });
}

/// The middle operand of `[receiver, first, second]` is protected across the
/// second operand's allocation, while the second operand has an empty suffix
/// and is reused directly. This pins the per-operand suffix computation rather
/// than merely the one-argument special case.
#[test]
fn dispatch_arguments_use_their_individual_collection_suffixes() {
    under_both_lowerings(|lowering| {
        let ir = main_ir_for_dispatch(
            "dispatch_argument_suffixes.ts",
            vec![allocating(), allocating()],
        );
        let alloc_results: Vec<String> = ir
            .lines()
            .map(str::trim)
            .filter_map(|line| line.split_once(" = "))
            .filter(|(_, instruction)| {
                instruction.starts_with("call ") && instruction.contains("@js_object_alloc(")
            })
            .map(|(result, _)| result.to_string())
            .collect();
        assert_eq!(
            alloc_results.len(),
            2,
            "{lowering}: fixture must emit exactly two allocating arguments:\n{ir}"
        );
        assert!(
            temp_root_slot_holding(&ir, &alloc_results[0]).is_some(),
            "{lowering}: first argument must be rooted across the second allocation:\n{ir}"
        );
        assert!(
            temp_root_slot_holding(&ir, &alloc_results[1]).is_none(),
            "{lowering}: final argument has an empty collection suffix and must not be rooted:\n{ir}"
        );
    });
}

/// #9479 covered the parallel known-class virtual/direct tower too. Its typed
/// receiver still spans an allocating argument, but that final argument is
/// consumed without another collection point and needs no slot of its own.
#[test]
fn known_class_dispatch_uses_the_same_suffix_windows() {
    under_both_lowerings(|lowering| {
        let heap = main_ir_for_known_dispatch("known_dispatch_heap_suffix.ts", allocating());
        let numeric =
            main_ir_for_known_dispatch("known_dispatch_numeric_suffix.ts", Expr::Integer(7));
        assert!(
            heap.contains("method_direct."),
            "{lowering}: fixture did not select the known-class guarded tower:\n{heap}"
        );
        let argument = first_call_result(&heap, "js_object_alloc")
            .unwrap_or_else(|| panic!("{lowering}: heap argument allocation missing:\n{heap}"));
        assert!(
            temp_root_slot_holding(&heap, &argument).is_none(),
            "{lowering}: final typed-dispatch argument must not be rooted:\n{heap}"
        );
        assert_eq!(
            temp_root_slots(&heap).len(),
            temp_root_slots(&numeric).len() + 1,
            "{lowering}: only the receiver window may add a slot to known-class dispatch"
        );
    });
}

/// Rest adaptation allocates after the source arguments are evaluated. That
/// explicitly extends every operand's window, including the final argument;
/// this is the safety control for #9480's narrowed ordinary-call case.
#[test]
fn rest_dispatch_keeps_the_post_argument_window() {
    under_both_lowerings(|lowering| {
        let ordinary = main_ir_for_dispatch("ordinary_dispatch_window.ts", vec![allocating()]);
        let ir = main_ir_for_dispatch_case(
            "rest_dispatch_window.ts",
            vec![allocating()],
            Type::Any,
            true,
        );
        let argument = first_call_result(&ir, "js_object_alloc")
            .unwrap_or_else(|| panic!("{lowering}: heap argument allocation missing:\n{ir}"));
        assert!(
            temp_root_slot_holding(&ir, &argument).is_some(),
            "{lowering}: rest-array construction follows the final argument, which must \
             remain rooted across it:\n{ir}"
        );
        assert_eq!(
            temp_root_slots(&ir).len(),
            temp_root_slots(&ordinary).len() + 1,
            "{lowering}: rest bundling must add the final argument's root"
        );
    });
}
