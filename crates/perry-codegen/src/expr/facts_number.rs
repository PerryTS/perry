//! CLS D2: Numbers proven by the whole-program facts, not by annotations.
//!
//! Two proofs, both about values, both fail-closed:
//!
//! * **A field read's kind.** A definite data birth field of prefix-stable
//!   classes (`wpfacts_access::prove`) whose F2 kind mask holds only
//!   primitives that `ToNumber` converts without running user code (Number,
//!   Boolean, null, undefined). Reading it and converting it is a load, a
//!   plain-number test and a cold leaf `js_number_coerce`; `+` of such a read
//!   with a Number is a Number (no string, BigInt or object can reach it).
//! * **A closed call's result.** A method call whose receiver the facts prove
//!   one of a closed class set, every class of which resolves the method to a
//!   cased prototype constant (the D1 site), and every resolved body returns a
//!   Number on every path that returns: literals, `Math.*`, arithmetic whose
//!   operand kinds exclude BigInt, `+` over Numbers and primitive-kind fields,
//!   and `this.m()` calls closed the same way. The body is judged under the
//!   receiver class set, so a subclass that stores a string into the field, or
//!   returns a non-Number from an override, fails the proof for its group.
//!
//! The value representation of a Number is its raw double: a consumer of a
//! proven Number takes the call's result as an `fadd` operand, with no
//! `js_dynamic_string_or_number_add`, no residual `js_number_coerce`.

use anyhow::Result;
use perry_hir::{BinaryOp, Expr, Stmt, UnaryOp};

use super::wpfacts_access::{self, Access};
use super::{lower_expr, FnCtx};
use crate::types::DOUBLE;

const K_NUM: u16 = 1;
const K_BOOL: u16 = 4;
const K_NULL: u16 = 8;
const K_UNDEF: u16 = 16;
const K_BIGINT: u16 = 32;
const K_OBJ: u16 = 128;
/// Kinds `ToNumber` converts with no user code and no allocation.
const K_PRIMS: u16 = K_NUM | K_BOOL | K_NULL | K_UNDEF;

/// How far a method-return proof follows `this.m()` calls.
const MAX_CALL_DEPTH: u8 = 4;

/// What the facts prove about an operand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Operand {
    /// A canonical raw double (a Number).
    Number,
    /// A field read holding Number, Boolean, null or undefined only.
    Prim,
    /// An element read the facts prove a Number or `undefined`
    /// (`facts_canonical::Num::NumberOrUndefined`).
    NumOrUndef,
}

/// The facts' answer for the element read `e` (`facts_canonical::of`): a
/// plain double at every copy of the read, possibly `undefined` for a hole
/// or an index past the end. The value is the facts' claim about whatever
/// the read yields, so the read itself is lowered as before; only the
/// conversion of its value is decided here.
fn element_operand(ctx: &FnCtx<'_>, e: &Expr) -> Option<Operand> {
    use crate::expr::facts_canonical::{of, Num};
    if !matches!(e, Expr::IndexGet { .. }) {
        return None;
    }
    let k = match of(ctx, e)? {
        Num::Number => Operand::Number,
        Num::NumberOrUndefined => Operand::NumOrUndef,
    };
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::CanonicalNumber,
        || serde_json::json!({ "arith": "element operand" }),
    );
    crate::facts_consumed::used();
    Some(k)
}

/// `e` is an element read the facts prove a Number or `undefined`: not a
/// BigInt. No recursion: a fact lookup.
pub(crate) fn element_read(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    element_operand(ctx, e).is_some()
}

/// The kind mask of the guard-free facts read `object.property`.
fn read_kinds(ctx: &FnCtx<'_>, object: &Expr, property: &str) -> Option<u16> {
    let p = wpfacts_access::prove(ctx, object, property, Access::Get)?;
    (!p.guarded() && p.kind_mask != 0).then_some(p.kind_mask)
}

/// Classify the caller-side operand `e`.
pub(crate) fn operand(ctx: &FnCtx<'_>, e: &Expr) -> Option<Operand> {
    // Inside a loop region's F copy, a bare read the region proves a Number
    // (R: an identity F64 lane, or the guard's value test) is a raw double.
    // That proof is stronger than the facts' kind mask, so the read must not
    // take the `Prim` lowering, whose `js_number_coerce` leaf is a call the
    // region verifier counts as one that may run JS.
    if crate::stmt::region_loop::is_f64_read(ctx, e) {
        return Some(Operand::Number);
    }
    if let Some(k) = element_operand(ctx, e) {
        return Some(k);
    }
    if let Expr::PropertyGet {
        object, property, ..
    } = e
    {
        if let Some(mask) = read_kinds(ctx, object, property) {
            if wpfacts_access::proves_number(ctx, e) {
                return Some(Operand::Number);
            }
            if mask & !K_PRIMS == 0 {
                return Some(Operand::Prim);
            }
            return None;
        }
    }
    // A nested `+` is classified by this module alone. Routing it through
    // `expr_produces_canonical_raw_f64` re-enters `add_is_number` and, when
    // the proof fails, `is_numeric_expr`, which descends again: two walks
    // per level of a left-nested chain, exponential in its length.
    if let Expr::Binary {
        op: BinaryOp::Add,
        left,
        right,
    } = e
    {
        return add_is_number(ctx, left, right).then_some(Operand::Number);
    }
    // `-` `*` `/` over two operands: `try_lower_arith` lowers it to one
    // IEEE operation on two Numbers (or, when both are already numeric, the
    // plain path converts them), so its value is a canonical double.
    if let Expr::Binary {
        op: BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div,
        left,
        right,
    } = e
    {
        if operand(ctx, left).is_some() && operand(ctx, right).is_some() {
            return Some(Operand::Number);
        }
    }
    crate::type_analysis::expr_produces_canonical_raw_f64(ctx, e).then_some(Operand::Number)
}

/// `left + right` is a Number by the facts: both operands are Numbers or
/// primitive-kind reads. Neither can be a string, a BigInt or an object, so
/// the sum is `ToNumber(left) + ToNumber(right)`.
pub(crate) fn add_is_number(ctx: &FnCtx<'_>, left: &Expr, right: &Expr) -> bool {
    operand(ctx, left).is_some() && operand(ctx, right).is_some()
}

/// The value of `e` is a Number, or `e` throws while it is evaluated: a
/// Number or primitive-kind operand (`operand`), or `-` `*` `/` `%` `**` over
/// two such values. Those operators apply ToNumeric to both operands
/// (13.15.3), which for Number, Boolean, null and undefined is a Number
/// (7.1.3), so the BigInt arm is never taken and the result is a Number.
pub(crate) fn kind_number(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::Binary {
            op: BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Pow,
            left,
            right,
        } => kind_number(ctx, left) && kind_number(ctx, right),
        _ => operand(ctx, e).is_some(),
    }
}

/// Every leaf of the `+` tree `e` is `kind_number`: the tree's ToPrimitive
/// and ToNumeric steps run no user code and cannot throw, and its value is a
/// Number.
pub(crate) fn add_tree_converts_silently(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } => add_tree_converts_silently(ctx, left) && add_tree_converts_silently(ctx, right),
        _ => kind_number(ctx, e),
    }
}

/// `e` is a guard-free facts read whose kind mask holds only Number, Boolean,
/// null and undefined. No recursion: a mask lookup.
pub(crate) fn prim_read(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    prim_read_mask(ctx, e).is_some()
}

fn prim_read_mask(ctx: &FnCtx<'_>, e: &Expr) -> Option<u16> {
    let Expr::PropertyGet {
        object, property, ..
    } = e
    else {
        return None;
    };
    read_kinds(ctx, object, property).filter(|m| m & !K_PRIMS == 0)
}

/// `left op right` for `-` `*` `/` whose operands are Numbers or
/// primitive-kind reads (`operand`): both are evaluated left to right, then
/// converted (ToNumeric of Number, Boolean, null and undefined is a Number,
/// 7.1.3; no user code, no BigInt arm, no throw), then one `fsub` / `fmul` /
/// `fdiv`. No dynamic helper.
///
/// A read whose mask is Number and undefined only converts with ONE test and
/// no branch: `select(is_number(v), v, NaN)`. The test is needed: `undefined`
/// is the NaN-boxed word `TAG_UNDEFINED` = 0x7FFC_0000_0000_0001
/// (`nanbox.rs`), a quiet NaN, and IEEE 754 propagates a NaN operand's
/// payload, so `fmul(x, undefined)` would return that very word: `undefined`,
/// not the Number NaN. A Number has one encoding in such a field (the plain
/// double, `FieldProof::number`), so every word that fails the plain-number
/// test is `undefined` and becomes the canonical NaN 0x7FF8_0000_0000_0000.
/// The result of an IEEE operation on plain doubles is a plain double (an
/// input NaN's payload, or a default NaN: both below the tag band; see
/// `emit_js_value_is_number`). A mask with Boolean or null keeps
/// `emit_prim_to_number` (its cold leaf `js_number_coerce`).
pub(crate) fn try_lower_arith(
    ctx: &mut FnCtx<'_>,
    op: BinaryOp,
    left: &Expr,
    right: &Expr,
) -> Result<Option<String>> {
    if !matches!(op, BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div) {
        return Ok(None);
    }
    let (Some(lk), Some(rk)) = (operand(ctx, left), operand(ctx, right)) else {
        return Ok(None);
    };
    if has_nan_leaf(ctx, left) || has_nan_leaf(ctx, right) {
        return lower_nan_tree_root(ctx, op, left, right).map(Some);
    }
    let l_raw = lower_expr(ctx, left)?;
    let l = arith_to_number(ctx, left, lk, &l_raw)?;
    let r_raw = lower_expr(ctx, right)?;
    let r = arith_to_number(ctx, right, rk, &r_raw)?;
    Ok(Some(ieee(ctx, op, &l, &r)))
}

fn ieee(ctx: &mut FnCtx<'_>, op: BinaryOp, l: &str, r: &str) -> String {
    let blk = ctx.block();
    match op {
        BinaryOp::Add => blk.fadd(l, r),
        BinaryOp::Sub => blk.fsub(l, r),
        BinaryOp::Mul => blk.fmul(l, r),
        _ => blk.fdiv(l, r),
    }
}

// ---------------------------------------------------------------------------
// Number-or-undefined trees
// ---------------------------------------------------------------------------
//
// A tree of `+ - * /` whose every node lowers here to one IEEE operation on
// the ToNumber values of its operands (`try_lower_arith`, `try_lower_add`:
// both operands `operand`), with leaves that are Numbers or reads the facts
// prove Number or `undefined`. ToNumber(undefined) is NaN (7.1.4), and
// Number::add / subtract / multiply / divide (6.1.6.1.3-7) of a NaN operand
// is NaN. `undefined` is the quiet-NaN word 0x7FFC_0000_0000_0001, and an
// IEEE 754 operation with a NaN operand returns a NaN (IEEE 754-2019 6.2),
// so a Number-or-undefined leaf enters the tree as it is: a value inside the
// tree is the right Number, or some NaN whose payload may lie in the tag
// band. No value inside the tree is observed: the root converts its result
// once, with the same plain-number test the leaf conversion used
// (`select(is_number(v), v, NaN)`): a word in the tag band is a NaN, the
// canonical NaN is the same Number. The test is on the bits (an integer
// compare), so it holds for whatever NaN payload LLVM gives the operation.
// One conversion per tree instead of one per leaf; the operations, their
// order and the evaluation order of the leaves (left to right; no
// conversion here runs code) are those of the per-node lowering.

/// The `+ - * /` node `e` this module lowers to one IEEE operation.
fn nan_tree_node<'e>(ctx: &FnCtx<'_>, e: &'e Expr) -> Option<(BinaryOp, &'e Expr, &'e Expr)> {
    let Expr::Binary { op, left, right } = e else {
        return None;
    };
    if !matches!(
        op,
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div
    ) {
        return None;
    }
    (operand(ctx, left).is_some() && operand(ctx, right).is_some()).then_some((
        *op,
        left.as_ref(),
        right.as_ref(),
    ))
}

/// The leaf `e` converts with the Number-or-undefined select of
/// `arith_to_number`.
fn nan_leaf(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match operand(ctx, e) {
        Some(Operand::NumOrUndef) => true,
        Some(Operand::Prim) => prim_read_mask(ctx, e).is_some_and(|m| m & !(K_NUM | K_UNDEF) == 0),
        _ => false,
    }
}

/// `e` is, or is a tree node over, a Number-or-undefined leaf.
fn has_nan_leaf(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match nan_tree_node(ctx, e) {
        Some((_, l, r)) => has_nan_leaf(ctx, l) || has_nan_leaf(ctx, r),
        None => nan_leaf(ctx, e),
    }
}

/// Lower `e` inside a tree (every node of it one IEEE operation, as the
/// per-node lowering would emit for Numbers): its value, and whether it may be a NaN word in
/// the tag band (a Number-or-undefined leaf reached it unconverted).
fn lower_nan_tree(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<(String, bool)> {
    if let Some((op, l, r)) = nan_tree_node(ctx, e) {
        let (lv, ln) = lower_nan_tree(ctx, l)?;
        let (rv, rn) = lower_nan_tree(ctx, r)?;
        return Ok((ieee(ctx, op, &lv, &rv), ln || rn));
    }
    if nan_leaf(ctx, e) {
        let raw = lower_expr(ctx, e)?;
        crate::expr::facts_canonical::witness(
            ctx,
            &raw,
            crate::expr::facts_canonical::Num::NumberOrUndefined,
            "arith tree operand",
        )?;
        return Ok((raw, true));
    }
    let k = operand(ctx, e).expect("a tree operand is classified");
    let raw = lower_expr(ctx, e)?;
    Ok((arith_to_number(ctx, e, k, &raw)?, false))
}

/// The root `left op right` of a tree with a Number-or-undefined leaf.
fn lower_nan_tree_root(
    ctx: &mut FnCtx<'_>,
    op: BinaryOp,
    left: &Expr,
    right: &Expr,
) -> Result<String> {
    let (lv, ln) = lower_nan_tree(ctx, left)?;
    let (rv, rn) = lower_nan_tree(ctx, right)?;
    let v = ieee(ctx, op, &lv, &rv);
    if !(ln || rn) {
        return Ok(v);
    }
    let is_num = crate::stmt::emit_js_value_is_number(ctx, &v);
    let nan = crate::nanbox::double_literal(f64::from_bits(0x7FF8_0000_0000_0000));
    Ok(ctx
        .block()
        .select(crate::types::I1, &is_num, DOUBLE, &v, &nan))
}

/// `ToNumber` of the operand `e` classified `k` (`operand`), whose lowered
/// value is `raw`. An element read proven a Number is witnessed in check
/// mode and taken as is; a Number-or-undefined element or field converts
/// with one test and no branch (the select of `try_lower_arith`'s doc: a
/// Number has one encoding there, `undefined` is a tag word, which must
/// become the canonical NaN, never propagate as a NaN payload).
pub(crate) fn arith_to_number(
    ctx: &mut FnCtx<'_>,
    e: &Expr,
    k: Operand,
    raw: &str,
) -> Result<String> {
    use crate::expr::facts_canonical::{witness, Num};
    let num_or_undef = match k {
        Operand::Number => {
            if matches!(e, Expr::IndexGet { .. }) && element_operand(ctx, e).is_some() {
                witness(ctx, raw, Num::Number, "arith element operand")?;
            }
            return Ok(raw.to_string());
        }
        Operand::NumOrUndef => true,
        Operand::Prim => prim_read_mask(ctx, e).is_some_and(|m| m & !(K_NUM | K_UNDEF) == 0),
    };
    if num_or_undef {
        witness(ctx, raw, Num::NumberOrUndefined, "arith operand")?;
        let is_num = crate::stmt::emit_js_value_is_number(ctx, raw);
        let nan = crate::nanbox::double_literal(f64::from_bits(0x7FF8_0000_0000_0000));
        return Ok(ctx
            .block()
            .select(crate::types::I1, &is_num, DOUBLE, raw, &nan));
    }
    Ok(emit_prim_to_number(ctx, raw))
}

/// `ToNumber` of a primitive-kind value: the plain-number case is the value,
/// every other case (undefined, null, a Boolean, an INT32 box) is the leaf
/// `js_number_coerce`.
pub(crate) fn emit_prim_to_number(ctx: &mut FnCtx<'_>, raw: &str) -> String {
    let is_num = crate::stmt::emit_js_value_is_number(ctx, raw);
    let fast_idx = ctx.new_block("facts_num.plain");
    let slow_idx = ctx.new_block("facts_num.coerce");
    let merge_idx = ctx.new_block("facts_num.merge");
    let fast_label = ctx.block_label(fast_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&is_num, &fast_label, &slow_label);
    ctx.current_block = fast_idx;
    ctx.block().br(&merge_label);
    ctx.current_block = slow_idx;
    let coerced = ctx
        .block()
        .call(DOUBLE, "js_number_coerce", &[(DOUBLE, raw)]);
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);
    ctx.current_block = merge_idx;
    ctx.block()
        .phi(DOUBLE, &[(raw, &fast_label), (&coerced, &slow_end)])
}

/// `left + right` over proven Numbers and primitive-kind reads: an `fadd`
/// of the two `ToNumber` values, no dynamic helper. Both operands are
/// evaluated left to right; neither conversion can run user code.
pub(crate) fn try_lower_add(
    ctx: &mut FnCtx<'_>,
    left: &Expr,
    right: &Expr,
) -> Result<Option<String>> {
    let (Some(lk), Some(rk)) = (operand(ctx, left), operand(ctx, right)) else {
        return Ok(None);
    };
    if has_nan_leaf(ctx, left) || has_nan_leaf(ctx, right) {
        return lower_nan_tree_root(ctx, BinaryOp::Add, left, right).map(Some);
    }
    // Number + Number is the existing numeric path.
    if lk == Operand::Number && rk == Operand::Number {
        return Ok(None);
    }
    let l_raw = lower_expr(ctx, left)?;
    let l = arith_to_number(ctx, left, lk, &l_raw)?;
    let r_raw = lower_expr(ctx, right)?;
    let r = arith_to_number(ctx, right, rk, &r_raw)?;
    Ok(Some(ctx.block().fadd(&l, &r)))
}

// ---------------------------------------------------------------------------
// Closed calls
// ---------------------------------------------------------------------------

/// The call `e` is a Number by the facts. Any call whose result the
/// whole-program facts prove a Number (`facts_rep::number_call`: a method
/// call, a direct call of a function, a call of any other callee value), or
/// the closed method call `receiver.m(...)` of the module doc.
pub(crate) fn closed_number_call(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    let Expr::Call {
        callee,
        byte_offset: call_offset,
        ..
    } = e
    else {
        return false;
    };
    if ctx.wpfacts_off {
        return false;
    }
    if crate::codegen::facts_rep::number_call(ctx, callee, *call_offset) {
        return true;
    }
    let Expr::PropertyGet {
        object, property, ..
    } = callee.as_ref()
    else {
        return false;
    };
    let Some((cids, _nullable)) = wpfacts_access::proven_receiver_classes(ctx, object) else {
        return false;
    };
    targets_return_number(ctx, &cids, property, 0)
}

/// One resolved implementor and the receiver classes that reach it.
struct Group {
    owner: String,
    function: String,
    cids: Vec<u32>,
}

fn targets_return_number(ctx: &FnCtx<'_>, cids: &[u32], property: &str, depth: u8) -> bool {
    if depth > MAX_CALL_DEPTH || cids.is_empty() || ctx.wpfacts.interp_guard {
        return false;
    }
    let mut groups: Vec<Group> = Vec::new();
    for &cid in cids {
        let Some(class) = wpfacts_access::class_name_of(ctx, cid) else {
            return false;
        };
        let mut cur = Some(class.clone());
        let mut found: Option<(String, String)> = None;
        while let Some(c) = cur {
            if let Some(f) = ctx.methods.get(&(c.clone(), property.to_string())) {
                found = Some((c, f.clone()));
                break;
            }
            cur = ctx.classes.get(&c).and_then(|cc| cc.extends_name.clone());
        }
        let Some((owner, function)) = found else {
            return false;
        };
        // The site's own admission: a prototype constant, not shadowed by a
        // field of that name.
        if !crate::lower_call::property_get::method_target_is_cased(
            ctx, &class, property, &function,
        ) {
            return false;
        }
        match groups.iter_mut().find(|g| g.function == function) {
            Some(g) => g.cids.push(cid),
            None => groups.push(Group {
                owner,
                function,
                cids: vec![cid],
            }),
        }
    }
    groups.iter().all(|g| {
        let Some(class) = ctx.classes.get(&g.owner) else {
            return false;
        };
        let Some(method) = class.methods.iter().find(|m| {
            m.name == property
                && ctx
                    .methods
                    .get(&(g.owner.clone(), m.name.clone()))
                    .is_some_and(|f| *f == g.function)
        }) else {
            return false;
        };
        !method.is_async
            && !method.is_generator
            && block_returns_number(ctx, &method.body, &g.cids, depth) == (true, true)
    })
}

/// `(every return is a Number, every normal path exits)` for the
/// straight-line / `if` subset; anything else rejects the proof.
fn block_returns_number(ctx: &FnCtx<'_>, stmts: &[Stmt], cids: &[u32], depth: u8) -> (bool, bool) {
    for stmt in stmts {
        match stmt {
            Stmt::Return(Some(expr), _) => return (expr_is_number(ctx, expr, cids, depth), true),
            Stmt::Return(None, _) => return (false, true),
            Stmt::Throw(_) => return (true, true),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                let (then_ok, then_exits) = block_returns_number(ctx, then_branch, cids, depth);
                let (else_ok, else_exits) = else_branch
                    .as_deref()
                    .map(|b| block_returns_number(ctx, b, cids, depth))
                    .unwrap_or((true, false));
                if !then_ok || !else_ok {
                    return (false, false);
                }
                if then_exits && else_exits {
                    return (true, true);
                }
            }
            Stmt::Let { .. } | Stmt::Expr(_) => {}
            _ => return (false, false),
        }
    }
    (true, false)
}

/// The kind mask of `this.key` over the receiver classes `cids`.
fn this_kinds(ctx: &FnCtx<'_>, cids: &[u32], key: &str) -> Option<u16> {
    let mut mask = 0u16;
    for &cid in cids {
        let f = ctx.wpfacts.field(cid, key)?;
        crate::verify_facts::consumed(
            crate::verify_facts::Fact::NumericResult,
            || serde_json::json!({"class":cid,"key":key,"mask":f.kind_mask}),
        );
        crate::facts_consumed::used();
        if f.declared || f.reserved {
            return None;
        }
        mask |= f.kind_mask;
    }
    (mask != 0).then_some(mask)
}

fn is_this_field(e: &Expr) -> Option<&str> {
    match e {
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::This) => Some(property.as_str()),
        _ => None,
    }
}

/// An expression whose value is a Number whatever the inputs, under the
/// receiver classes `cids`.
fn expr_is_number(ctx: &FnCtx<'_>, e: &Expr, cids: &[u32], depth: u8) -> bool {
    match e {
        Expr::Integer(_) | Expr::Number(_) | Expr::NumberCoerce(_) | Expr::DateNow => true,
        Expr::Binary { op, left, right } => match op {
            BinaryOp::Add => {
                prim_or_number(ctx, left, cids, depth) && prim_or_number(ctx, right, cids, depth)
            }
            BinaryOp::UShr => true,
            // A BigInt result needs two BigInt operands: one non-BigInt
            // operand makes it a Number or a throw.
            _ => non_bigint(ctx, left, cids, depth) || non_bigint(ctx, right, cids, depth),
        },
        Expr::Unary { op, operand } => match op {
            UnaryOp::Pos => true,
            UnaryOp::Neg | UnaryOp::BitNot => non_bigint(ctx, operand, cids, depth),
            UnaryOp::Not => false,
        },
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            expr_is_number(ctx, then_expr, cids, depth)
                && expr_is_number(ctx, else_expr, cids, depth)
        }
        Expr::Call { callee, .. } => match callee.as_ref() {
            Expr::PropertyGet {
                object, property, ..
            } if matches!(object.as_ref(), Expr::This) => {
                targets_return_number(ctx, cids, property, depth + 1)
            }
            _ => false,
        },
        _ => {
            if is_ctx_free_math(e) {
                return true;
            }
            is_this_field(e)
                .and_then(|k| this_kinds(ctx, cids, k))
                .is_some_and(|m| m == K_NUM)
        }
    }
}

/// Number, or a primitive-kind field read.
fn prim_or_number(ctx: &FnCtx<'_>, e: &Expr, cids: &[u32], depth: u8) -> bool {
    expr_is_number(ctx, e, cids, depth)
        || is_this_field(e)
            .and_then(|k| this_kinds(ctx, cids, k))
            .is_some_and(|m| m & !K_PRIMS == 0)
}

/// Provably not a BigInt (and not an object whose conversion could yield one).
fn non_bigint(ctx: &FnCtx<'_>, e: &Expr, cids: &[u32], depth: u8) -> bool {
    expr_is_number(ctx, e, cids, depth)
        || is_this_field(e)
            .and_then(|k| this_kinds(ctx, cids, k))
            .is_some_and(|m| m & (K_BIGINT | K_OBJ | 64) == 0)
}

fn is_ctx_free_math(e: &Expr) -> bool {
    matches!(
        e,
        Expr::MathFloor(..)
            | Expr::MathCeil(..)
            | Expr::MathRound(..)
            | Expr::MathTrunc(..)
            | Expr::MathSign(..)
            | Expr::MathAbs(..)
            | Expr::MathSqrt(..)
            | Expr::MathLog(..)
            | Expr::MathLog2(..)
            | Expr::MathLog10(..)
            | Expr::MathPow(..)
            | Expr::MathMin(..)
            | Expr::MathMax(..)
            | Expr::MathImul(..)
            | Expr::MathSin(..)
            | Expr::MathCos(..)
            | Expr::MathTan(..)
            | Expr::MathAtan2(..)
            | Expr::MathExp(..)
    )
}
