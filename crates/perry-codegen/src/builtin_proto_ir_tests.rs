//! #11915: IR census for the builtin-prototype `Pristine` proof. Every direct
//! builtin lowering on a proven or kind-tested receiver must test the shadow
//! bit of the agent's intrinsic prototype (object model step 3: the layout
//! word compared with `UNSHADOWED_WORD_END`) on the way in, and its failure
//! must reach the ONE shared generic path — never a per-site fallback body.

use perry_hir::types::Type;
use perry_hir::{ArrayLikeSource, Expr, Stmt};

use crate::temp_root_coverage::main_ir_for;

/// The shadow-bit compare: `icmp uge i32 %word, UNSHADOWED_WORD_END`.
const PATCHED: &str = "-1879048192";
const _: () = assert!(crate::runtime_abi::LAYOUT_ORDINARY_END as i32 == -1879048192);

fn let_local(id: u32, name: &str, ty: Type, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty,
        mutable: false,
        init: Some(init),
    }
}

fn array_literal() -> Expr {
    Expr::Array(vec![Expr::Number(1.0), Expr::Number(2.0)], None)
}

#[test]
fn a_folded_array_method_is_one_checked_entry_call() {
    let ir = main_ir_for(
        "pristine_folded.ts",
        vec![
            let_local(1, "a", Type::Array(Box::new(Type::Number)), array_literal()),
            Stmt::Expr(Expr::ArrayJoin {
                array: Box::new(Expr::LocalGet(1)),
                separator: Some(Box::new(Expr::String("-".to_string()))),
            }),
        ],
    );
    assert_eq!(
        ir.matches("call double @js_array_m_join1(").count(),
        1,
        "the fold is ONE call into the checked entry, which proves \
         Pristine(Array) and the receiver's own proof itself:\n{ir}"
    );
    assert!(
        !ir.contains(PATCHED) && !ir.contains("call double @js_builtin_proto_generic_call"),
        "no test and no generic arm at the site:\n{ir}"
    );
}

#[test]
fn a_folded_array_method_with_an_observing_argument_tests_before_it() {
    // `a.map(o.f)`: reading `o.f` can run a getter, which could modify
    // `Array.prototype` after the method was read; the site decides first.
    let ir = main_ir_for(
        "pristine_folded_observing.ts",
        vec![
            let_local(1, "a", Type::Array(Box::new(Type::Number)), array_literal()),
            let_local(2, "o", Type::Any, Expr::Object(Vec::new())),
            Stmt::Expr(Expr::ArrayMap {
                array: Box::new(Expr::LocalGet(1)),
                callback: Box::new(Expr::PropertyGet {
                    byte_offset: 0,
                    object: Box::new(Expr::LocalGet(2)),
                    property: "f".to_string(),
                }),
            }),
        ],
    );
    assert!(
        ir.contains(PATCHED) && !ir.contains("call double @js_array_m_map1("),
        "an observing argument reads the fact before it:\n{ir}"
    );
    assert!(
        ir.contains("call double @js_builtin_proto_generic_call1(")
            && ir.contains("call double @js_array_b_map1("),
        "then a failed proof is ONE generic call and a held one the unchecked \
         body, on the arguments evaluated once:\n{ir}"
    );
}

#[test]
fn a_borrowed_prototype_call_proves_array_and_function_and_a_binding_does_not() {
    let call = |source| {
        Stmt::Expr(Expr::ArrayLikeMethod {
            method: "slice".to_string(),
            receiver: Box::new(Expr::LocalGet(1)),
            args: Vec::new(),
            source,
        })
    };
    let proto = main_ir_for(
        "pristine_borrowed_proto.ts",
        vec![
            let_local(1, "o", Type::Any, Expr::Object(Vec::new())),
            call(ArrayLikeSource::Prototype),
        ],
    );
    // Each family's proof is its own intrinsic's shadow bit OR'd with
    // `%Object.prototype%`'s (every family inherits it): Array (agent slot
    // INTRINSIC_PROTO+2) and Function (+1) each add the shared Object word.
    assert_eq!(
        proto.matches(PATCHED).count(),
        4,
        "a prototype read proves Array (byte 2) and Function (byte 1), each \
         with %Object.prototype%:\n{proto}"
    );
    for slot in ["i64 48", "i64 40", "i64 32"] {
        assert!(
            proto.contains(&format!("getelementptr i8, ptr @PERRY_AGENT_PTRS, {slot}")),
            "the Array, Function and Object intrinsics are all tested ({slot}):\n{proto}"
        );
    }
    assert!(
        proto.contains("@js_builtin_proto_generic_call("),
        "and its generic arm reads %Array.prototype%:\n{proto}"
    );
    let binding = main_ir_for(
        "pristine_borrowed_binding.ts",
        vec![
            let_local(1, "o", Type::Any, Expr::Object(Vec::new())),
            call(ArrayLikeSource::Binding),
        ],
    );
    assert!(
        !binding.contains(PATCHED),
        "a captured binding holds its value; no proof is needed:\n{binding}"
    );
}

#[test]
fn a_proven_string_method_is_one_checked_entry_call() {
    let ir = main_ir_for(
        "pristine_string.ts",
        vec![
            let_local(1, "s", Type::String, Expr::String("abcdef".to_string())),
            Stmt::Expr(Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    byte_offset: 0,
                    object: Box::new(Expr::LocalGet(1)),
                    property: "slice".to_string(),
                }),
                args: vec![Expr::Number(1.0), Expr::Number(3.0)],
                type_args: Vec::new(),
                byte_offset: 0,
            }),
        ],
    );
    assert_eq!(
        ir.matches("call double @js_string_m_slice2(").count(),
        1,
        "{ir}"
    );
    assert!(
        !ir.contains(PATCHED),
        "the entry proves Pristine(String):\n{ir}"
    );
    assert!(
        !ir.contains("@js_string_slice(") && !ir.contains("@js_string_index_to_i32("),
        "the unbox / coerce / call sequence moved into the entry:\n{ir}"
    );
}
