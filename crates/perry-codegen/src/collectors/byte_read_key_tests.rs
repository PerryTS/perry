//! #7700: the key-kind condition on `Expr::Uint8ArrayGet`.
//!
//! These are unit tests on purpose. The acceptance case is
//! `test-files/test_gap_uint8array_nonnumeric_key_7700.ts`, and the gap suite
//! is TAG-gated — a regression there would sit red for days (#5960). The
//! predicate is `--lib`-visible, so this file runs on every PR.

use super::byte_read_key::uint8array_get_reads_a_byte;
use perry_hir::{BinaryOp, Expr, UnaryOp};

/// No per-local evidence — the purely structural answer.
fn structural(index: &Expr) -> bool {
    uint8array_get_reads_a_byte(index, &mut |_| false)
}

/// Local `7` is proven numeric; nothing else is.
fn with_numeric_local_7(index: &Expr) -> bool {
    uint8array_get_reads_a_byte(index, &mut |id| id == 7)
}

fn sym(key: &str) -> Expr {
    Expr::SymbolFor(Box::new(Expr::String(key.to_string())))
}

fn bin(op: BinaryOp, l: Expr, r: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(l),
        right: Box::new(r),
    }
}

#[test]
fn numeric_literals_read_a_byte() {
    assert!(structural(&Expr::Integer(3)));
    assert!(structural(&Expr::Number(3.0)));
}

/// The regression itself: `const it = u8[Symbol.iterator]` lowers to a
/// `Uint8ArrayGet` with a `SymbolFor` key. Answering "byte read" here gave the
/// destination local an i32 slot, and `ToInt32(ToNumber(fn))` made `typeof it`
/// report `number` instead of `function`.
#[test]
fn a_symbol_key_does_not_read_a_byte() {
    assert!(!structural(&sym("@@__perry_wk_iterator")));
    assert!(!with_numeric_local_7(&sym("@@__perry_wk_iterator")));
}

/// `const k: any = "byteLength"; u8[k]` — the key is a `LocalGet` the caller
/// cannot prove numeric, so this is a property read, not a byte read.
#[test]
fn an_unproven_local_key_does_not_read_a_byte() {
    assert!(!structural(&Expr::LocalGet(0)));
    assert!(!with_numeric_local_7(&Expr::LocalGet(0)));
}

/// …and a string key never did.
#[test]
fn a_string_key_does_not_read_a_byte() {
    assert!(!structural(&Expr::String("subarray".to_string())));
}

/// The hot shape must survive: `for (let i = …) sum += buf[i]`. A local the
/// caller has proven integer-valued IS numeric-key evidence — the read is a
/// byte at runtime whichever codegen path it takes — so the loop keeps its i32
/// representation.
#[test]
fn a_proven_numeric_local_key_reads_a_byte() {
    assert!(with_numeric_local_7(&Expr::LocalGet(7)));
    assert!(with_numeric_local_7(&Expr::Update {
        id: 7,
        op: perry_hir::UpdateOp::Increment,
        prefix: false,
    }));
    // `buf[i + 1]`, `buf[i & 7]`.
    assert!(with_numeric_local_7(&bin(
        BinaryOp::Add,
        Expr::LocalGet(7),
        Expr::Integer(1)
    )));
    assert!(with_numeric_local_7(&bin(
        BinaryOp::BitAnd,
        Expr::LocalGet(7),
        Expr::Integer(7)
    )));
}

/// ToInt32/ToUint32 producers are numeric whatever the operands are, so an
/// unproven local under a mask still reads a byte — `buf[k & 0xff]` is the
/// idiom this must not deoptimize.
#[test]
fn a_masked_unproven_key_still_reads_a_byte() {
    assert!(structural(&bin(
        BinaryOp::BitAnd,
        Expr::LocalGet(0),
        Expr::Integer(255)
    )));
    assert!(structural(&bin(
        BinaryOp::Shr,
        Expr::LocalGet(0),
        Expr::Integer(2)
    )));
}

/// `+` may be string concatenation, so BOTH sides must be numeric — otherwise
/// `u8["by" + "teLength"]` would be admitted as a byte read.
#[test]
fn add_needs_both_sides_numeric() {
    assert!(!structural(&bin(
        BinaryOp::Add,
        Expr::LocalGet(0),
        Expr::Integer(1)
    )));
    assert!(structural(&bin(
        BinaryOp::Add,
        Expr::Integer(2),
        Expr::Integer(1)
    )));
    assert!(!structural(&bin(
        BinaryOp::Add,
        Expr::String("by".to_string()),
        Expr::String("teLength".to_string())
    )));
}

/// `-x` is ToNumber — except on a BigInt, which stays a BigInt. The operand
/// has to be numeric.
#[test]
fn unary_needs_a_numeric_operand() {
    assert!(structural(&Expr::Unary {
        op: UnaryOp::Neg,
        operand: Box::new(Expr::Integer(1)),
    }));
    assert!(!structural(&Expr::Unary {
        op: UnaryOp::Neg,
        operand: Box::new(Expr::LocalGet(0)),
    }));
    // `!x` is a boolean, not a number.
    assert!(!structural(&Expr::Unary {
        op: UnaryOp::Not,
        operand: Box::new(Expr::Integer(1)),
    }));
}

/// A nested byte read is a number, so it is a numeric key — but only if ITS
/// own key is numeric.
#[test]
fn a_nested_byte_read_is_a_numeric_key() {
    let inner_numeric = Expr::Uint8ArrayGet {
        array: Box::new(Expr::LocalGet(1)),
        index: Box::new(Expr::Integer(0)),
    };
    assert!(structural(&inner_numeric));

    let inner_symbol = Expr::Uint8ArrayGet {
        array: Box::new(Expr::LocalGet(1)),
        index: Box::new(sym("@@__perry_wk_iterator")),
    };
    assert!(!structural(&inner_symbol));
}

/// `u8.length` / `Math.floor(x)` are numbers.
#[test]
fn lengths_and_math_are_numeric_keys() {
    assert!(structural(&Expr::Uint8ArrayLength(Box::new(
        Expr::LocalGet(1)
    ))));
    assert!(structural(&Expr::MathFloor(Box::new(Expr::LocalGet(0)))));
}

/// The allowlist's default: an expression kind nobody enumerated is NOT a
/// numeric key. This is the property the blocklist it replaced did not have —
/// "not a string literal" put every unconsidered key kind, symbols first, on
/// the wrong side.
#[test]
fn an_unenumerated_key_kind_is_rejected() {
    assert!(!structural(&Expr::PropertyGet {
        byte_offset: 0,
        object: Box::new(Expr::LocalGet(0)),
        property: "k".to_string(),
    }));
    assert!(!structural(&Expr::Undefined));
    assert!(!structural(&Expr::Null));
    assert!(!structural(&Expr::Bool(true)));
}
