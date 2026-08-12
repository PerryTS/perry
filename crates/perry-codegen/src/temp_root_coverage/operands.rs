//! #6996: the rooting gate must not root values that cannot be heap
//! references — and must still root the ones that can.
//!
//! `native_abi_packet_control`'s kernel is `(buf[i] + packet.tag + i) & 255`.
//! The left operand is a byte; the right is a property get on an `any`, so the
//! add lowers through `js_dynamic_string_or_number_add` and the operand pair is
//! rooted across the property get. Rooting the byte protected nothing and cost
//! a store, a re-read and a clear on every one of the loop's 262 144
//! iterations — it turned the `hot_loops_no_runtime_calls` contract red.
//!
//! The two directions are stated as a matched pair so neither can pass for
//! nothing: the same fixture, differing only in the element key, must root in
//! one arm and not in the other.

use super::{main_ir_for, under_both_lowerings};
use crate::testing::temp_slots::{assert_no_temp_rooting, temp_root_slots};
use perry_hir::{Expr, Stmt};

/// A local declared with a Buffer annotation, so the receiver of the element
/// read is a plain local get (the shape the fixture's parameter has).
fn buffer_local(id: u32) -> Stmt {
    Stmt::Let {
        id,
        name: "buf".to_string(),
        ty: perry_hir::types::Type::Named("Buffer".to_string()),
        mutable: false,
        init: Some(Expr::Undefined),
    }
}

/// An `any` local, so a property get on it is neither numeric-proven nor
/// allocation-free — exactly what forces the dynamic add and its rooting.
fn any_local(id: u32, name: &str) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: perry_hir::types::Type::Any,
        mutable: false,
        init: Some(Expr::Undefined),
    }
}

fn packet_tag(id: u32) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(id)),
        property: "tag".to_string(),
        byte_offset: 0,
    }
}

/// `<element read> + packet.tag`, bound to a local so nothing folds it away.
fn dynamic_add_with_element_read(element: Expr, packet_id: u32) -> Stmt {
    Stmt::Let {
        id: 90,
        name: "next".to_string(),
        ty: perry_hir::types::Type::Number,
        mutable: false,
        init: Some(Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left: Box::new(element),
            right: Box::new(packet_tag(packet_id)),
        }),
    }
}

fn element_read_fixture(name: &str, element: Expr, extra: Vec<Stmt>) -> String {
    let mut init = vec![buffer_local(0), any_local(1, "packet")];
    init.extend(extra);
    init.push(dynamic_add_with_element_read(element, 1));
    main_ir_for(name, init)
}

/// A proven-index typed-array element read is a byte (or `undefined` out of
/// range) by construction, so it must not be rooted.
#[test]
fn a_proven_index_typed_array_read_is_not_temp_rooted() {
    under_both_lowerings(|lowering| {
        let ir = element_read_fixture(
            "typed_element_operand.ts",
            Expr::Uint8ArrayGet {
                array: Box::new(Expr::LocalGet(0)),
                index: Box::new(Expr::Integer(0)),
            },
            Vec::new(),
        );
        assert!(
            ir.contains("call double @js_dynamic_string_or_number_add"),
            "{lowering}: the fixture must actually reach the rooted \
             operand-pair lowering, or this proves nothing:\n{ir}"
        );
        assert_no_temp_rooting(&ir, lowering);
    });
}

/// Same for a `Buffer`-node element read, whose every lowering coerces the key
/// to i32 and reads a byte.
#[test]
fn a_buffer_index_read_is_not_temp_rooted() {
    under_both_lowerings(|lowering| {
        let ir = element_read_fixture(
            "buffer_index_operand.ts",
            Expr::BufferIndexGet {
                buffer: Box::new(Expr::LocalGet(0)),
                index: Box::new(Expr::Integer(0)),
            },
            Vec::new(),
        );
        assert_no_temp_rooting(&ir, lowering);
    });
}

/// The soundness boundary, held from the other side: a symbol key does NOT
/// read an element. It resolves through `js_object_get_symbol_property`, which
/// hands back a `%TypedArray%.prototype` accessor — a heap value an allocating
/// sibling can sweep. It must still be rooted.
///
/// This is the differential control for the two negatives above: same fixture,
/// same shape, one changed key.
#[test]
fn a_symbol_keyed_typed_array_read_is_still_temp_rooted() {
    under_both_lowerings(|lowering| {
        let ir = element_read_fixture(
            "typed_symbol_key_operand.ts",
            Expr::Uint8ArrayGet {
                array: Box::new(Expr::LocalGet(0)),
                index: Box::new(Expr::SymbolFor(Box::new(Expr::String(
                    "Symbol.iterator".to_string(),
                )))),
            },
            Vec::new(),
        );
        assert!(
            !temp_root_slots(&ir).is_empty(),
            "{lowering}: a symbol-keyed read returns a prototype accessor, not \
             a byte — the #6996 skip must not reach it:\n{ir}"
        );
    });
}

/// The other excluded lowering: without the integer-array-index proof the read
/// goes to `js_typed_array_index_get_dynamic`, which falls through to
/// string-keyed property lookup, and an expando can hold anything.
#[test]
fn an_unproven_key_typed_array_read_is_still_temp_rooted() {
    under_both_lowerings(|lowering| {
        let ir = element_read_fixture(
            "typed_unproven_key_operand.ts",
            Expr::Uint8ArrayGet {
                array: Box::new(Expr::LocalGet(0)),
                index: Box::new(Expr::LocalGet(2)),
            },
            vec![any_local(2, "k")],
        );
        assert!(
            !temp_root_slots(&ir).is_empty(),
            "{lowering}: an unproven key reads a property, not an element — the \
             #6996 skip must not reach it:\n{ir}"
        );
    });
}
