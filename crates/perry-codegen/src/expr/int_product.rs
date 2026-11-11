//! A relational or equality test over an integer product, sum or difference
//! is one integer compare, in the width the intervals prove.
//!
//! `piece * color > 0` over two int32 values was `sitofp` of both, `fmul`,
//! `fcmp` against `0.0`. The int32 chain (`i32_fast_path`, `i32_temps`) does
//! not take it: the product of two int32 values is not an int32 (`-2^31 *
//! -1`), and LLVM cannot fold the `fmul` because it has no bound of the
//! operands. The value is still an integer the intervals bound:
//!
//! - a **leaf** is an int32 Number at every evaluation with no side effect:
//!   an int32 literal, an exact int32 chain (`i32_temps::exact`: an I32 slot,
//!   a range-proven local, a selection of those), or a word the facts hold
//!   (`int32_operand::word_read`: an I32 lane, an at-rest element, an int32
//!   element read). Its interval is the facts' (`range_facts::int_range_expr`,
//!   which reads `BindingFacts::int_range` for a proven int32 `let`), or the
//!   whole int32 range;
//! - a **tree** is `+`, `-`, `*` or `Math.abs` over leaves and trees. Every
//!   operand of an operator is exact (inside `[-2^53, 2^53]`, by interval
//!   arithmetic), so the double the operator makes is the correctly rounded
//!   exact integer, and that integer fits an i64.
//!
//! A test `l <op> r` (`<`, `<=`, `>`, `>=`, `==`, `===`, `!=`, `!==`) with a
//! tree on a side is lowered as integers when both sides are exact, or when
//! one side's root may round (its interval leaves `[-2^53, 2^53]` but fits an
//! i64) and the other side is strictly inside `(-2^53, 2^53)`: rounding to
//! nearest is monotone and maps no integer past `2^53` to one below it, so
//! the order against such a value is the exact integers' order. `a * b > 0`
//! over any two int32 values is this case. A test absorbs the sign of a zero
//! (`-0 > 0`, `-0 === 0`), and int32 operands make no NaN or Infinity, so the
//! integer compare is the JavaScript result. The width is i32 when every
//! interval of the test is inside int32, i64 otherwise; the operators carry
//! `nsw` (the intervals prove it).
//!
//! The **initializer of an I32 local** (`stmt/let_stmt.rs`: the facts made
//! the local an int32) that is a tree whose every interval is inside int32
//! is stored as its i32 ([`try_lower_i32`]): `const type = Math.abs(p)` with
//! `p` in `[-6, 6]` is the integer absolute value, not `sitofp`, a mask and
//! `fptosi`. The store's truncation maps `-0` to `0` as the integer does.
//!
//! Not taken: a tree whose operand may leave `[-2^53, 2^53]` (the double
//! computation rounds inside the tree), two rounding sides, a leaf that may
//! write (`i++`, a call), a fractional, NaN or unproven operand, and any
//! value use of the tree (its `-0` is observable there).
//!
//! Probe: `PERRY_ARRAY_BASE_DIAG=1` prints one `[perry intprod]` line per
//! test with a tree side. Check mode (`PERRY_VERIFY_FACTS=1`): each leaf is
//! tested against the interval the proof used (exit 86,
//! `Fact::Int32Input`). Negative control: `PERRY_FACTS_SABOTAGE=intprod`
//! claims every non-literal leaf is `0`.
//!
//! `PERRY_INT_PRODUCTS=0` (`off`, `false`): main's lowering.
//! `PERRY_INT_PRODUCTS=products`: tests over `+`, `-` and `*` only (no
//! `Math.abs` node, no I32 initializer).

use anyhow::Result;
use perry_hir::{BinaryOp, CompareOp, Expr, LogicalOp};

use super::FnCtx;
use crate::types::{DOUBLE, I1, I32, I64};

/// Integers of magnitude up to this are exact doubles.
const EXACT: i128 = 1 << 53;

pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_INT_PRODUCTS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// `PERRY_INT_PRODUCTS=products`: only `+`, `-` and `*` under a test; no
/// `Math.abs` node and no int32 store ([`try_lower_i32`]).
fn abs_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("PERRY_INT_PRODUCTS").as_deref() != Ok("products"))
}

fn sabotaged() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("PERRY_FACTS_SABOTAGE").as_deref() == Ok("intprod"))
}

type Iv = (i128, i128);

const INT32: Iv = (i32::MIN as i128, i32::MAX as i128);

fn within(a: Iv, b: Iv) -> bool {
    a.0 >= b.0 && a.1 <= b.1
}

enum Node<'a> {
    Leaf {
        e: &'a Expr,
        /// Lowered by `int32_operand` (a word the facts hold).
        word: bool,
        iv: Iv,
    },
    Op {
        op: BinaryOp,
        left: Box<Node<'a>>,
        right: Box<Node<'a>>,
        iv: Iv,
    },
    /// `Math.abs` of an exact operand.
    Abs { inner: Box<Node<'a>>, iv: Iv },
}

impl Node<'_> {
    fn iv(&self) -> Iv {
        match self {
            Node::Leaf { iv, .. } | Node::Op { iv, .. } | Node::Abs { iv, .. } => *iv,
        }
    }

    fn is_tree(&self) -> bool {
        !matches!(self, Node::Leaf { .. })
    }

    /// Every interval of the tree is inside int32.
    fn narrow(&self) -> bool {
        match self {
            Node::Leaf { iv, .. } => within(*iv, INT32),
            Node::Op {
                left, right, iv, ..
            } => within(*iv, INT32) && left.narrow() && right.narrow(),
            Node::Abs { inner, iv } => within(*iv, INT32) && inner.narrow(),
        }
    }
}

/// `e` reads locals and typed-array elements under operators and
/// selections: evaluating it writes nothing and runs no user code, so the
/// intervals taken before the test is lowered hold at every leaf.
fn simple(e: &Expr) -> bool {
    match e {
        Expr::Integer(_) | Expr::LocalGet(_) | Expr::This => true,
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => simple(left) && simple(right),
        Expr::Unary { operand, .. } => simple(operand),
        Expr::MathAbs(x) => simple(x),
        Expr::MathMin(args) | Expr::MathMax(args) => args.iter().all(simple),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => simple(condition) && simple(then_expr) && simple(else_expr),
        Expr::IndexGet { object, index, .. } => {
            matches!(object.as_ref(), Expr::LocalGet(_)) && simple(index)
        }
        _ => false,
    }
}

/// `this`, a local, or a field path from one.
fn place(e: &Expr) -> bool {
    match e {
        Expr::This | Expr::LocalGet(_) => true,
        Expr::PropertyGet { object, .. } => place(object),
        _ => false,
    }
}

/// The shape of a word read whose operands write nothing: `this.f`, `a[k]`
/// and `a[k] ?? n` over simple operands. Whether it is a word is
/// `int32_operand::word_read`'s proof.
fn simple_word(e: &Expr) -> bool {
    match e {
        Expr::Logical {
            op: LogicalOp::Coalesce,
            left,
            right,
        } => simple_word(left) && simple(right),
        Expr::PropertyGet { .. } => place(e),
        Expr::IndexGet { object, index, .. } => place(object) && simple(index),
        _ => false,
    }
}

fn leaf<'a>(ctx: &FnCtx<'_>, e: &'a Expr) -> Option<Node<'a>> {
    let literal = matches!(e, Expr::Integer(n) if i32::try_from(*n).is_ok());
    let word = !literal && simple_word(e) && super::int32_operand::word_read(ctx, e);
    let chain = !word
        && simple(e)
        && super::can_lower_expr_as_i32_in_current_region(ctx, e)
        && super::i32_temps::exact(ctx, e);
    if !word && !chain {
        return None;
    }
    let iv = if sabotaged() && !literal {
        (0, 0)
    } else {
        super::range_facts::int_range_expr(ctx, e)
            .map(|r| {
                (
                    i128::from(r.min).max(INT32.0),
                    i128::from(r.max).min(INT32.1),
                )
            })
            .filter(|iv| iv.0 <= iv.1)
            .unwrap_or(INT32)
    };
    Some(Node::Leaf { e, word, iv })
}

/// The leaf or tree of `e` (module docs), or the reason there is none.
fn node<'a>(ctx: &FnCtx<'_>, e: &'a Expr, depth: u32) -> Result<Node<'a>, &'static str> {
    if let Some(l) = leaf(ctx, e) {
        return Ok(l);
    }
    if depth >= 16 {
        return Err("depth");
    }
    if let Expr::MathAbs(x) = e {
        if !abs_enabled() {
            return Err("leaf");
        }
        let inner = node(ctx, x, depth + 1)?;
        let a = inner.iv();
        if !within(a, (-EXACT, EXACT)) {
            return Err("operand-past-2^53");
        }
        let iv = if a.0 >= 0 {
            a
        } else if a.1 <= 0 {
            (-a.1, -a.0)
        } else {
            (0, (-a.0).max(a.1))
        };
        return Ok(Node::Abs {
            inner: Box::new(inner),
            iv,
        });
    }
    let Expr::Binary { op, left, right } = e else {
        return Err("leaf");
    };
    if !matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul) {
        return Err("leaf");
    }
    let l = node(ctx, left, depth + 1)?;
    let r = node(ctx, right, depth + 1)?;
    let (a, b) = (l.iv(), r.iv());
    if !within(a, (-EXACT, EXACT)) || !within(b, (-EXACT, EXACT)) {
        return Err("operand-past-2^53");
    }
    let iv = match op {
        BinaryOp::Add => (a.0 + b.0, a.1 + b.1),
        BinaryOp::Sub => (a.0 - b.1, a.1 - b.0),
        _ => {
            let p = [a.0 * b.0, a.0 * b.1, a.1 * b.0, a.1 * b.1];
            (
                p.iter().copied().min().unwrap_or(0),
                p.iter().copied().max().unwrap_or(0),
            )
        }
    };
    if !within(iv, (i128::from(i64::MIN), i128::from(i64::MAX))) {
        return Err("past-i64");
    }
    Ok(Node::Op {
        op: *op,
        left: Box::new(l),
        right: Box::new(r),
        iv,
    })
}

fn diag(ctx: &FnCtx<'_>, op: CompareOp, l: Option<Iv>, r: Option<Iv>, verdict: &str) {
    if std::env::var("PERRY_ARRAY_BASE_DIAG").as_deref() == Ok("1") {
        let iv = |v: Option<Iv>| v.map_or("?".to_string(), |(lo, hi)| format!("[{lo},{hi}]"));
        eprintln!(
            "[perry intprod] fn={} op={op:?} left={} right={} verdict={verdict}",
            ctx.func.name,
            iv(l),
            iv(r)
        );
    }
}

/// Check mode: the leaf's i32 `value` is inside the interval the proof used.
fn witness(ctx: &mut FnCtx<'_>, value: &str, iv: Iv) {
    if !crate::verify_facts::enabled() {
        return;
    }
    let blk = ctx.block();
    let lo = blk.icmp_sge(I32, value, &iv.0.to_string());
    let hi = blk.icmp_sle(I32, value, &iv.1.to_string());
    let ok = blk.and(I1, &lo, &hi);
    crate::verify_facts::tick(ctx);
    let miss = ctx.new_block("intprod.verify_miss");
    let next = ctx.new_block("intprod.admitted");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    ctx.block().cond_br(&ok, &next_l, &miss_l);
    ctx.current_block = miss;
    let as_double = ctx.block().sitofp(I32, value, DOUBLE);
    let site = crate::verify_facts::site_arg(
        ctx,
        "int product",
        crate::verify_facts::Fact::Int32Input,
        &[],
    );
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, &as_double)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
}

/// The integer of `n`, an i64 when `wide`, else an i32.
fn lower(ctx: &mut FnCtx<'_>, n: &Node<'_>, wide: bool) -> Result<String> {
    match n {
        Node::Leaf { e, word, iv } => {
            if let Expr::Integer(k) = e {
                return Ok(k.to_string());
            }
            let v = if *word {
                super::int32_operand::lower_word(ctx, e)?
            } else {
                super::lower_expr_as_i32(ctx, e)?
            };
            witness(ctx, &v, *iv);
            Ok(if wide {
                ctx.block().sext(I32, &v, I64)
            } else {
                v
            })
        }
        Node::Op {
            op, left, right, ..
        } => {
            // Left to right: each operand is an integer once lowered.
            let l = lower(ctx, left, wide)?;
            let r = lower(ctx, right, wide)?;
            let name = match op {
                BinaryOp::Add => "add",
                BinaryOp::Sub => "sub",
                _ => "mul",
            };
            let ty = if wide { "i64" } else { "i32" };
            let blk = ctx.block();
            let out = blk.fresh_reg();
            blk.emit_raw(format!("{out} = {name} nsw {ty} {l}, {r}"));
            Ok(out)
        }
        Node::Abs { inner, .. } => {
            // The operand's interval excludes the type's minimum (it is
            // exact, and in the i32 width `narrow` bounds its absolute value).
            let v = lower(ctx, inner, wide)?;
            let ty = if wide { I64 } else { I32 };
            let name = if wide { "i64" } else { "i32" };
            let blk = ctx.block();
            let neg = blk.fresh_reg();
            blk.emit_raw(format!("{neg} = sub nsw {name} 0, {v}"));
            let below = blk.icmp_slt(ty, &v, "0");
            Ok(blk.select(I1, &below, ty, &neg, &v))
        }
    }
}

/// Only an expression with an operator at its root can be a tree.
fn arith(e: &Expr) -> bool {
    matches!(
        e,
        Expr::MathAbs(_)
            | Expr::Binary {
                op: BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul,
                ..
            }
    )
}

/// The i32 of `e` for the store into an I32 local when `e` is a tree whose
/// every interval is inside int32 (module docs); `None` leaves it to the
/// other lowerings.
pub(crate) fn try_lower_i32(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<Option<String>> {
    if !enabled() || !abs_enabled() || !arith(e) {
        return Ok(None);
    }
    let Ok(n) = node(ctx, e, 0) else {
        return Ok(None);
    };
    if !n.is_tree() || !n.narrow() {
        return Ok(None);
    }
    if std::env::var("PERRY_ARRAY_BASE_DIAG").as_deref() == Ok("1") {
        let (lo, hi) = n.iv();
        eprintln!(
            "[perry intprod] fn={} value=[{lo},{hi}] verdict=i32-value",
            ctx.func.name
        );
    }
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::Int32Input,
        || serde_json::json!({ "operand": "int product" }),
    );
    crate::facts_consumed::used();
    Ok(Some(lower(ctx, &n, false)?))
}

/// `left <op> right` as one integer compare (an `i1`) when a side is an
/// integer tree the intervals bound (module docs); `None` leaves the test to
/// the other lowerings.
pub(crate) fn try_lower_compare(
    ctx: &mut FnCtx<'_>,
    op: CompareOp,
    left: &Expr,
    right: &Expr,
) -> Result<Option<String>> {
    if !enabled() {
        return Ok(None);
    }
    if !arith(left) && !arith(right) {
        return Ok(None);
    }
    let (l, r) = match (node(ctx, left, 0), node(ctx, right, 0)) {
        (Ok(l), Ok(r)) => (l, r),
        (l, r) => {
            let why = l.as_ref().err().or(r.as_ref().err()).copied().unwrap_or("");
            diag(
                ctx,
                op,
                l.ok().map(|n| n.iv()),
                r.ok().map(|n| n.iv()),
                &format!("refused:{why}"),
            );
            return Ok(None);
        }
    };
    // Two exact int32 chains are `i32_temps`' compare.
    if !l.is_tree() && !r.is_tree() {
        return Ok(None);
    }
    let (a, b) = (l.iv(), r.iv());
    let exact = |v: Iv| within(v, (-EXACT, EXACT));
    let inside = |v: Iv| within(v, (-EXACT + 1, EXACT - 1));
    let rounds = !(exact(a) && exact(b));
    if rounds && !inside(a) && !inside(b) {
        diag(ctx, op, Some(a), Some(b), "refused:rounding");
        return Ok(None);
    }
    let wide = !(l.narrow() && r.narrow());
    diag(
        ctx,
        op,
        Some(a),
        Some(b),
        match (wide, rounds) {
            (false, _) => "i32",
            (true, false) => "i64",
            (true, true) => "i64-rounded",
        },
    );
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::Int32Input,
        || serde_json::json!({ "operand": "int product" }),
    );
    crate::facts_consumed::used();
    let lv = lower(ctx, &l, wide)?;
    let rv = lower(ctx, &r, wide)?;
    let ty = if wide { I64 } else { I32 };
    let blk = ctx.block();
    Ok(Some(match op {
        CompareOp::Lt => blk.icmp_slt(ty, &lv, &rv),
        CompareOp::Le => blk.icmp_sle(ty, &lv, &rv),
        CompareOp::Gt => blk.icmp_sgt(ty, &lv, &rv),
        CompareOp::Ge => blk.icmp_sge(ty, &lv, &rv),
        CompareOp::Eq | CompareOp::LooseEq => blk.icmp_eq(ty, &lv, &rv),
        CompareOp::Ne | CompareOp::LooseNe => blk.icmp_ne(ty, &lv, &rv),
    }))
}
