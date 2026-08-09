//! Rooting coverage for the windows #7615 slice 8 closed.
//!
//! # What is asserted, and why it cannot pass vacuously
//!
//! The harness is slice 7's, reused rather than copied — the same
//! "the register the consuming call reads must be DEFINED BELOW the last
//! allocation above the call" ordering property, and the same rule that every
//! test first proves by callee name that the arm under test was reached at all.
//!
//! **Ordering, never slot counts, and the reason is sharper here than in slice
//! 7.** A slot-width assertion silently measures nothing on the DEFAULT build:
//! under statepoints `reserve_shadow_slot` hands back a stack-map index and no
//! `js_shadow_frame_enter` is emitted at all, so a test that counts frame slots
//! reads zero and passes. The definition-line ordering is visible in all three
//! lowerings — pooled alloca, shadow frame and FFI fallback — because all three
//! must produce the re-read below the window or they are not rooting anything.
//!
//! **The zero-cost arm is a test too.** `[1,2].map(f)` where `f` is inert
//! cannot collect between the receiver and `js_array_map`, so
//! `operand_protection` must answer `Reuse` and emit nothing. Without that pin
//! a future "root everything" change would tax every call site unnoticed.
//!
//! # ★ The receiver is a PROPERTY READ, and that is load-bearing
//!
//! The first version of these tests used an array-typed LOCAL as the receiver,
//! and the sabotage arm — restore the pre-fix lowering, require red — came back
//! green. The reason is worth more than the tests: for a local with a shadow
//! slot, codegen's `ptr addrspace(1)` retype pass **rematerialises the load
//! from that slot at the use site**, so the pre-fix code re-read the receiver
//! by accident and had no window at all. Measured on the perry-dev A/B: the
//! baseline arm emits `%rN.rs4p = load ptr addrspace(1), ptr %slot` BELOW the
//! callback's allocation, all by itself.
//!
//! The windows that are real — verified by A/B on emitted IR — are the
//! receivers with no slot to rematerialise from: a MODULE GLOBAL, a CLASS
//! FIELD read, and a CLOSURE CAPTURE (`js_closure_get_capture_bits`, a raw
//! `i64`, #7280 taxonomy (a)). These tests use the field read, because it is
//! the one a single HIR function can express.

use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module as HirModule, Stmt};

use super::slice7_rooting_tests::{allocating, require_call_line, temp_root_calls};

/// Instructions that move bits around without reading memory, calling anything
/// or joining control flow — so a register defined by one of these is only as
/// fresh as ITS input.
const PURE_OPS: &[&str] = &[
    "bitcast",
    "and ",
    "or ",
    "xor ",
    "shl ",
    "lshr",
    "ashr",
    "ptrtoint",
    "inttoptr",
    "zext",
    "sext",
    "trunc",
    "add ",
    "sub ",
    "getelementptr",
    "select",
];

/// Walk `reg`'s definition chain up through [`PURE_OPS`] and return the line of
/// the first instruction that actually PRODUCES a value — a call, a load, a phi
/// or an argument.
///
/// ★ **This is the whole reason this file does not reuse slice 7's
/// `assert_operand_survives_the_window`.** That helper compares the operand
/// register's own definition line against the window, and for
/// `Expr::ArrayMap` that is a `and i64 %stale, POINTER_MASK` — which the
/// pre-fix lowering emitted BELOW the window while masking a register loaded
/// ABOVE it. Measured, not reasoned: the first version of these tests used the
/// shallow helper, and the sabotage arm (restore the pre-fix lowering, require
/// red) came back GREEN on all four. "The unbox sits below its own window" is
/// exactly the shape a one-level check cannot see, which is #7280 taxonomy (c)
/// stated as a property of the instrument instead of the bug.
pub(super) fn producer_line(ir: &str, reg: &str) -> usize {
    let lines: Vec<&str> = ir.lines().collect();
    let mut current = reg.to_string();
    for _ in 0..32 {
        let prefix = format!("{current} = ");
        let Some(idx) = lines
            .iter()
            .position(|l| l.trim_start().starts_with(&prefix))
        else {
            panic!("no definition for {current} in:\n{ir}");
        };
        let rhs = lines[idx].split_once(" = ").expect("matched on ' = '").1;
        let Some(op) = PURE_OPS
            .iter()
            .find(|op| rhs.trim_start().starts_with(**op))
        else {
            return idx;
        };
        // Follow the FIRST register operand of the pure op.
        let Some(next) = rhs[op.len()..]
            .split(&[',', ' '][..])
            .find(|t| t.starts_with('%'))
        else {
            return idx; // constant-only pure op: it is its own producer
        };
        current = next.trim_end_matches(')').to_string();
    }
    panic!("definition chain for {reg} did not terminate in:\n{ir}");
}

/// Line index of a call to `callee` that is not a `declare`.
pub(super) fn call_line_of(ir: &str, callee: &str) -> usize {
    let needle = format!("@{callee}(");
    ir.lines()
        .position(|l| l.contains(&needle) && !l.trim_start().starts_with("declare"))
        .unwrap_or_else(|| panic!("no call to {callee} in:\n{ir}"))
}

/// The `n`-th SSA operand of the call to `callee`.
pub(super) fn call_operand_of(ir: &str, callee: &str, n: usize) -> String {
    let idx = call_line_of(ir, callee);
    let line = ir.lines().nth(idx).expect("index came from this IR");
    let args = line
        .rsplit_once('(')
        .unwrap_or_else(|| panic!("{callee} call has no argument list: {line}"))
        .1;
    args.split(',')
        .nth(n)
        .unwrap_or_else(|| panic!("{callee} has no operand {n}: {line}"))
        .trim()
        .rsplit(' ')
        .next()
        .expect("an operand is a type followed by a register")
        .trim_end_matches(')')
        .to_string()
}

/// Assert that the value `consumer` reads as operand `consumer_n` was PRODUCED
/// **below** the value `window` reads as operand `window_n`.
///
/// The window is named by the LATER OPERAND'S OWN PRODUCER rather than by "the
/// last object allocation above the call". Which helper an `Expr::Object`
/// lowering reaches for is not a property of this module —
/// `js_object_alloc_with_shape`, an inline bump and
/// `js_object_alloc_class_inline_keys` are all reachable, and the last two emit
/// no `@js_object_alloc` line at all — so an allocation anchor is a guess about
/// somebody else's lowering. The operand's own producer is not: whatever the
/// callback's lowering emitted, the register it hands to the call is produced
/// by it, and the receiver must be re-read below that.
fn assert_reread_below_operand(
    ir: &str,
    consumer: &str,
    consumer_n: usize,
    window: &str,
    window_n: usize,
    what: &str,
) {
    let reg = call_operand_of(ir, consumer, consumer_n);
    let producer = producer_line(ir, &reg);
    let window_reg = call_operand_of(ir, window, window_n);
    let window_line = producer_line(ir, &window_reg);
    assert!(
        producer > window_line,
        "{what}: {consumer} reads {reg}, whose value is PRODUCED at line {producer} — at or \
         above line {window_line}, where {window}'s operand {window_n} ({window_reg}) is \
         produced. That lowering is the window. Masking or bit-casting the receiver below it \
         does not repair it: the bits are still the pre-move address. It must be rooted above \
         the window and re-read below it.\n{ir}"
    );
}

/// Compile a one-function module and return its LLVM IR.
///
/// A local copy rather than slice 7's, because these arms need a *local* — the
/// receiver of `arr.map(cb)` has to be something `lower_expr` can lower, and a
/// `LocalGet` needs the slot the declaration creates.
fn compile_body(name: &str, body: Vec<Stmt>) -> String {
    let mut hir = HirModule::new(name);
    hir.functions.push(Function {
        id: 0,
        name: "build".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let opts = crate::CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    let bytes = crate::compile_module(&hir, opts).expect("test module compiles");
    String::from_utf8(bytes).expect("LLVM IR is UTF-8")
}

/// `let o = { items: [...] }; <tail>` — the receiver reached through a FIELD
/// read, which is a call result with no slot for the retype pass to
/// rematerialise from. See the module header for why a local will not do.
fn with_object_local(id: u32, tail: Stmt) -> Vec<Stmt> {
    vec![
        Stmt::Let {
            id,
            name: "o".to_string(),
            ty: Type::Any,
            init: Some(Expr::Object(vec![(
                "items".to_string(),
                Expr::Array(vec![Expr::Number(1.0), Expr::Number(2.0)]),
            )])),
            mutable: false,
        },
        tail,
    ]
}

/// `o.items` — the receiver expression these tests use.
fn field_receiver(id: u32) -> Expr {
    Expr::PropertyGet {
        byte_offset: 0,
        object: Box::new(Expr::LocalGet(id)),
        property: "items".to_string(),
    }
}

// ---------------------------------------------------------------------------
// expr/math_simple.rs — Expr::ArrayMap
// ---------------------------------------------------------------------------

/// The live bug slice 8 fixed.
///
/// `Expr::ArrayMap` lowered the receiver, lowered the callback, and only THEN
/// unboxed the receiver — so `unbox_to_i64` sat BELOW its own window and masked
/// a stale box rather than repairing it (#7280 taxonomy (c): an
/// operand-to-operand window). The receiver handle `js_array_map` reads must
/// therefore be defined below the callback's allocation.
#[test]
fn array_map_receiver_is_reread_below_the_callback_lowering() {
    let ir = compile_body(
        "array_map_window",
        with_object_local(
            1,
            Stmt::Expr(Expr::ArrayMap {
                array: Box::new(field_receiver(1)),
                callback: Box::new(allocating("cb")),
            }),
        ),
    );
    assert_reread_below_operand(
        &ir,
        "js_array_map",
        0,
        "js_validate_array_map_callback",
        1,
        "arr.map(cb) evaluates the receiver first and the callback second, and the \
         callback's lowering allocates",
    );
}

/// The same window, one instruction earlier: `js_validate_array_map_callback`
/// takes the receiver too, and it is emitted from the same re-read. Asserting
/// both is what stops a "fix" that re-reads for `js_array_map` only and leaves
/// the validator dereferencing from-space.
#[test]
fn array_map_validator_reads_the_same_reread_receiver() {
    let ir = compile_body(
        "array_map_validator",
        with_object_local(
            1,
            Stmt::Expr(Expr::ArrayMap {
                array: Box::new(field_receiver(1)),
                callback: Box::new(allocating("cb")),
            }),
        ),
    );
    assert_reread_below_operand(
        &ir,
        "js_validate_array_map_callback",
        0,
        "js_validate_array_map_callback",
        1,
        "the non-callable validator dereferences the receiver as well",
    );
}

/// The zero-cost arm: an inert callback cannot collect, so nothing is rooted
/// and the IR is what it was before the fix.
#[test]
fn array_map_with_an_inert_callback_emits_no_rooting_traffic() {
    let ir = compile_body(
        "array_map_cold",
        with_object_local(
            1,
            Stmt::Expr(Expr::ArrayMap {
                array: Box::new(field_receiver(1)),
                callback: Box::new(Expr::Undefined),
            }),
        ),
    );
    require_call_line(&ir, "js_array_map");
    assert_eq!(
        temp_root_calls(&ir),
        0,
        "an inert callback cannot collect between the receiver and js_array_map, so \
         operand_protection must route the receiver to Reuse\n{ir}"
    );
}

// ---------------------------------------------------------------------------
// expr/math_simple.rs — Expr::MapGet / Expr::MapHas
// ---------------------------------------------------------------------------

/// `m.get(k)` lowers the receiver before the key, and an allocating key is a
/// window the receiver must survive. This is the `with_operands_rooted`
/// translation of what `lower_operand_pair_rooted` did, pinned so the
/// translation cannot quietly drop the root.
#[test]
fn map_get_receiver_survives_an_allocating_key() {
    let ir = compile_body(
        "map_get_window",
        with_object_local(
            1,
            Stmt::Expr(Expr::MapGet {
                map: Box::new(field_receiver(1)),
                key: Box::new(allocating("k")),
            }),
        ),
    );
    assert_reread_below_operand(
        &ir,
        "js_map_get",
        0,
        "js_map_get",
        1,
        "Map.get evaluates the receiver first and the key second",
    );
}
