use std::collections::{HashMap, HashSet};

use super::*;

/// An integer literal only proves an int32 value when it *is* one (#6319).
///
/// `Expr::Integer` carries an `i64`, so it happily holds `3000000000` or
/// `9007199254740991` — values that are integral but do NOT fit in i32. Every
/// judgment in the i32 fast-path chain used to accept `Expr::Integer(_)`
/// unconditionally, which is how a literal past 2^31 reached an i32 shadow
/// slot and got silently `ToInt32`-wrapped (`3000000000` → `-1294967296`).
///
/// This is the one predicate all four of those judgments now share:
/// `is_strictly_i32_bounded_expr` (the write-is-i32-bounded proof),
/// `collect_integer_let_ids` (the syntactic seed), `is_int32_producing_expr`
/// (the forward-closure admission) and `int32_producing_deps` (the
/// disqualification judgment). Narrowing only the seed would not have held:
/// the forward closure re-admits from the init, and the judgment re-admits
/// from every `LocalSet` rhs.
///
/// Deliberately *not* widened to "any value ToInt32 can wrap". `| 0`, `>>> 0`,
/// bitwise ops and `Math.imul` all wrap by spec, so their results are genuinely
/// int32 and keep their own arms below; a bare literal does not wrap — `let x =
/// 3000000000` is the Number 3000000000 and must stay a double.
pub(crate) fn integer_literal_fits_i32(n: i64) -> bool {
    i32::try_from(n).is_ok()
}

/// Is `e` a write whose value provably fits in i32?
///
/// `known_i32_ranged_locals` is the *oracle*: locals already proven to hold an
/// i32-ranged value. Only the `LocalGet` and `Update` arms consult it — every
/// other arm is a self-contained proof (a small literal, an explicit `| 0` /
/// `>>> 0` coercion, a bitwise op, `Math.imul`, a clamp call, a byte load).
/// That split is what keeps the oracle's greatest fixpoint (#6339) away from
/// intentionally-i32 code: an FNV / `imul32` / bit-mixing chain proves itself
/// and never asks the oracle anything, so shrinking the oracle cannot cost it
/// its i32 slot.
///
/// Every oracle lookup the verdict *relied on* is reported through `on_dep`, so
/// judgment and provenance come from one match and can never drift apart: a new
/// oracle-consulting arm is automatically both judged and tracked. Callers that
/// only want the verdict pass `&mut |_| {}`.
pub fn is_strictly_i32_bounded_expr(
    e: &perry_hir::Expr,
    known_i32_ranged_locals: &HashSet<u32>,
    flat_const_ids: &HashSet<u32>,
    flat_row_alias_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
    on_dep: &mut dyn FnMut(u32),
) -> bool {
    use perry_hir::{BinaryOp, Expr};
    match e {
        // #6319: an out-of-i32-range literal is NOT i32-bounded. Without the
        // magnitude check, `let y = 0; y = 3000000000;` counted every write to
        // `y` as strict, `y` got an i32 shadow at its `Let` site, and the store
        // truncated to -1294967296.
        Expr::Integer(n) => integer_literal_fits_i32(*n),
        // `x++`/`x--` is i32-bounded ONLY when the updated local is itself
        // i32-ranged (a loop counter). The previous unconditional `true`
        // truncated `var y = x++` to an i32 slot even when `x` held a
        // fractional/non-number value — so `var x = 1.1; var y = x++` stored
        // `y = (i32)1.1 = 1` instead of the spec's coerced old value `1.1`.
        Expr::Update { id, .. } => {
            let ok = known_i32_ranged_locals.contains(id);
            if ok {
                on_dep(*id);
            }
            ok
        }
        // `expr | 0` / `expr >>> 0` ToInt32/ToUint32 idioms — explicit i32
        // coercion, hard-bounded.
        Expr::Binary { op, right, .. }
            if matches!(op, BinaryOp::BitOr | BinaryOp::UShr)
                && matches!(right.as_ref(), Expr::Integer(0)) =>
        {
            true
        }
        // Pure bitwise — always i32 per JS spec.
        Expr::Binary { op, .. } => matches!(
            op,
            BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr
                | BinaryOp::UShr
        ),
        Expr::Call { callee, .. } => {
            if let Expr::FuncRef(fid) = callee.as_ref() {
                clamp_fn_ids.contains(fid)
            } else {
                false
            }
        }
        // #6339: a plain copy is i32-bounded only if the *source* is proven
        // i32-RANGED. Answering this from `integer_locals` (merely
        // integer-VALUED, which by design admits overflowing `Add`/`Sub`/`Mul`)
        // is what let `let big2 = big1 + big1; t = big2;` truncate `t`.
        Expr::LocalGet(id) => {
            let ok = known_i32_ranged_locals.contains(id);
            if ok {
                on_dep(*id);
            }
            ok
        }
        Expr::Uint8ArrayGet { .. } | Expr::BufferIndexGet { .. } => true,
        Expr::MathImul(_, _) => true,
        Expr::IndexGet { object, .. } => match object.as_ref() {
            Expr::IndexGet { object: inner, .. } => {
                matches!(inner.as_ref(), Expr::LocalGet(id) if flat_const_ids.contains(id))
            }
            Expr::LocalGet(id) => flat_row_alias_ids.contains(id),
            _ => false,
        },
        _ => false,
    }
}

/// Output of the strict-write walk.
///
/// `saw_any` / `disqualified` are the per-local verdicts. `copy_deps` is the
/// #6339 provenance: `copy_deps[d]` lists the locals whose *strict* verdict for
/// some write relied on the oracle vouching for `d` (i.e. the write was a copy
/// `id = d` / `id = d++`). Disqualifying `d` must therefore disqualify them too.
#[derive(Default)]
pub struct StrictWriteFacts {
    pub saw_any: HashSet<u32>,
    pub disqualified: HashSet<u32>,
    copy_deps: HashMap<u32, Vec<u32>>,
}

/// Judge one write to `id` against the oracle and fold the verdict into `out`.
fn record_strict_write(
    id: u32,
    value: &perry_hir::Expr,
    known_i32_ranged: &HashSet<u32>,
    flat_const_ids: &HashSet<u32>,
    flat_row_alias_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
    out: &mut StrictWriteFacts,
) {
    out.saw_any.insert(id);
    let mut deps: Vec<u32> = Vec::new();
    let strict = is_strictly_i32_bounded_expr(
        value,
        known_i32_ranged,
        flat_const_ids,
        flat_row_alias_ids,
        clamp_fn_ids,
        &mut |d| deps.push(d),
    );
    if !strict {
        out.disqualified.insert(id);
        return;
    }
    // Strict — but conditionally so. Record the copy edges the verdict leaned
    // on; `collect_strictly_i32_bounded_locals` revokes it if a source falls.
    for d in deps {
        if d != id {
            out.copy_deps.entry(d).or_default().push(id);
        }
    }
}

/// Locals where every write (including the `Stmt::Let` init) is proven to fit
/// in i32 (issue #436). These are correctness-safe on the i32 fast path even
/// when they're never used as an array index — the bounded writes guarantee the
/// value fits by construction rather than by induction over loop iterations.
///
/// Used to extend the Let-site `needs_i32_slot` gate beyond `index_used_locals`.
/// Image_convolution's FNV-1a `h` accumulator is the motivating shape: writes
/// are `(h ^ dst[i]) | 0` (explicit `| 0` coerce) and `imul32(h, K)`
/// (returns_integer call) — both strict — so `h` qualifies even though it's
/// never used as an index. #435's `sum`, `prod`, etc. write through bare
/// `Add | Sub | Mul` and stay out.
///
/// ## Greatest fixpoint (#6339 — fourth face of #6072)
///
/// The set is defined by mutual recursion: a copy `t = big2` is strict iff
/// `big2` is i32-ranged, which is this very set. The oracle used to be
/// `integer_locals`, which is a *different* property — integer-VALUED, and by
/// its own doc deliberately admits overflowing `Add`/`Sub`/`Mul`. So
///
/// ```text
/// let big1 = 2000000000;
/// let big2 = big1 + big1;   // 4e9: integer-valued, NOT i32-ranged
/// let t = 0; t = big2;      // judged strict -> i32 shadow -> -294967296
/// ```
///
/// truncated the copy while `big2` itself (whose `Add` write is not strict)
/// stayed correct. The cure is to seed the oracle optimistically with
/// `integer_locals` and take the GREATEST fixpoint of "every write is strict":
/// `is_strictly_i32_bounded_expr` is monotone in the oracle, so starting at the
/// top and only ever removing is exact and terminating.
///
/// It is computed in one walk rather than by re-walking per removal. The
/// judgment is shallow and only two of its arms consult the oracle (`LocalGet`,
/// `Update`), so each strict write records the exact local it leaned on; a
/// worklist then propagates every disqualification backwards along those edges.
///
/// Locals that lose the shadow fall back to their f64 slot — correct, and only
/// marginally slower. Index-used locals keep it through `index_used_locals`,
/// an independent term in `needs_i32_slot`, so the numeric array-index fast
/// path (#6299/#6312) and the loop counters (#6318) are untouched.
pub fn collect_strictly_i32_bounded_locals(
    stmts: &[perry_hir::Stmt],
    integer_locals: &HashSet<u32>,
    flat_const_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
) -> HashSet<u32> {
    let mut flat_row_alias_ids: HashSet<u32> = HashSet::new();
    collect_flat_row_aliases(stmts, flat_const_ids, &mut flat_row_alias_ids);

    // Optimistic seed: assume every integer-valued local is also i32-ranged,
    // then let the walk record which writes actually prove it and which merely
    // borrowed the assumption.
    let mut out = StrictWriteFacts::default();
    walk_writes_for_strict(
        stmts,
        integer_locals,
        flat_const_ids,
        &flat_row_alias_ids,
        clamp_fn_ids,
        &mut out,
    );

    // Shrink to the fixpoint: a local with a non-strict write is not i32-ranged,
    // and neither is anything that was only strict because it copied one.
    let mut worklist: Vec<u32> = out.disqualified.iter().copied().collect();
    while let Some(fallen) = worklist.pop() {
        let Some(dependents) = out.copy_deps.get(&fallen) else {
            continue;
        };
        for dependent in dependents.clone() {
            if out.disqualified.insert(dependent) {
                worklist.push(dependent);
            }
        }
    }

    out.saw_any
        .iter()
        .copied()
        .filter(|id| !out.disqualified.contains(id))
        .collect()
}

/// Mutable locals whose observable value is always produced by a top-level
/// `>>> 0` cast. They cannot join signed `integer_locals` because normal
/// reads must see a u32-as-double value, not a signed i32. Codegen still keeps
/// a parallel i32 bit-pattern slot for hot bitwise consumers, and converts
/// that slot back with `uitofp` for ordinary JS reads.
pub fn collect_unsigned_i32_locals(stmts: &[perry_hir::Stmt]) -> HashSet<u32> {
    let mut saw_any: HashSet<u32> = HashSet::new();
    let mut disqualified: HashSet<u32> = HashSet::new();
    walk_writes_for_unsigned_i32(stmts, &mut saw_any, &mut disqualified);
    saw_any
        .into_iter()
        .filter(|id| !disqualified.contains(id))
        .collect()
}

fn walk_writes_for_unsigned_i32(
    stmts: &[perry_hir::Stmt],
    saw_any: &mut HashSet<u32>,
    disqualified: &mut HashSet<u32>,
) {
    use perry_hir::Stmt;
    for s in stmts {
        match s {
            Stmt::Let {
                id,
                init: Some(init),
                mutable,
                ..
            } => {
                if *mutable {
                    saw_any.insert(*id);
                    if !is_ushr_zero(init) {
                        disqualified.insert(*id);
                    }
                }
                walk_writes_in_expr_for_unsigned_i32(init, saw_any, disqualified);
            }
            Stmt::Let { init: None, .. } => {}
            Stmt::Expr(e) | Stmt::Throw(e) => {
                walk_writes_in_expr_for_unsigned_i32(e, saw_any, disqualified);
            }
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    walk_writes_in_expr_for_unsigned_i32(e, saw_any, disqualified);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                walk_writes_in_expr_for_unsigned_i32(condition, saw_any, disqualified);
                walk_writes_for_unsigned_i32(then_branch, saw_any, disqualified);
                if let Some(eb) = else_branch {
                    walk_writes_for_unsigned_i32(eb, saw_any, disqualified);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                walk_writes_in_expr_for_unsigned_i32(condition, saw_any, disqualified);
                walk_writes_for_unsigned_i32(body, saw_any, disqualified);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    walk_writes_for_unsigned_i32(
                        std::slice::from_ref(init_stmt),
                        saw_any,
                        disqualified,
                    );
                }
                if let Some(cond) = condition {
                    walk_writes_in_expr_for_unsigned_i32(cond, saw_any, disqualified);
                }
                if let Some(upd) = update {
                    walk_writes_in_expr_for_unsigned_i32(upd, saw_any, disqualified);
                }
                walk_writes_for_unsigned_i32(body, saw_any, disqualified);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                walk_writes_for_unsigned_i32(body, saw_any, disqualified);
                if let Some(c) = catch {
                    walk_writes_for_unsigned_i32(&c.body, saw_any, disqualified);
                }
                if let Some(f) = finally {
                    walk_writes_for_unsigned_i32(f, saw_any, disqualified);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                walk_writes_in_expr_for_unsigned_i32(discriminant, saw_any, disqualified);
                for c in cases {
                    if let Some(t) = &c.test {
                        walk_writes_in_expr_for_unsigned_i32(t, saw_any, disqualified);
                    }
                    walk_writes_for_unsigned_i32(&c.body, saw_any, disqualified);
                }
            }
            Stmt::Labeled { body, .. } => {
                walk_writes_for_unsigned_i32(
                    std::slice::from_ref(body.as_ref()),
                    saw_any,
                    disqualified,
                );
            }
            _ => {}
        }
    }
}

fn walk_writes_in_expr_for_unsigned_i32(
    e: &perry_hir::Expr,
    saw_any: &mut HashSet<u32>,
    disqualified: &mut HashSet<u32>,
) {
    use perry_hir::Expr;
    match e {
        Expr::LocalSet(id, value) => {
            saw_any.insert(*id);
            if !is_ushr_zero(value) {
                disqualified.insert(*id);
            }
            walk_writes_in_expr_for_unsigned_i32(value, saw_any, disqualified);
        }
        Expr::Update { id, .. } => {
            saw_any.insert(*id);
            disqualified.insert(*id);
        }
        _ => {
            perry_hir::walker::walk_expr_children(e, &mut |child| {
                walk_writes_in_expr_for_unsigned_i32(child, saw_any, disqualified);
            });
        }
    }
}

pub fn walk_writes_for_strict(
    stmts: &[perry_hir::Stmt],
    known_i32_ranged: &HashSet<u32>,
    flat_const_ids: &HashSet<u32>,
    flat_row_alias_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
    out: &mut StrictWriteFacts,
) {
    use perry_hir::Stmt;
    for s in stmts {
        match s {
            Stmt::Let {
                id,
                init: Some(init),
                ..
            } => {
                record_strict_write(
                    *id,
                    init,
                    known_i32_ranged,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                    out,
                );
                walk_writes_in_expr_for_strict(
                    init,
                    known_i32_ranged,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                    out,
                );
            }
            Stmt::Let { init: None, .. } => {}
            Stmt::Expr(e) | Stmt::Throw(e) => {
                walk_writes_in_expr_for_strict(
                    e,
                    known_i32_ranged,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                    out,
                );
            }
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    walk_writes_in_expr_for_strict(
                        e,
                        known_i32_ranged,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                        out,
                    );
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                walk_writes_in_expr_for_strict(
                    condition,
                    known_i32_ranged,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                    out,
                );
                walk_writes_for_strict(
                    then_branch,
                    known_i32_ranged,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                    out,
                );
                if let Some(eb) = else_branch {
                    walk_writes_for_strict(
                        eb,
                        known_i32_ranged,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                        out,
                    );
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                walk_writes_in_expr_for_strict(
                    condition,
                    known_i32_ranged,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                    out,
                );
                walk_writes_for_strict(
                    body,
                    known_i32_ranged,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                    out,
                );
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    walk_writes_for_strict(
                        std::slice::from_ref(init_stmt),
                        known_i32_ranged,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                        out,
                    );
                }
                if let Some(cond) = condition {
                    walk_writes_in_expr_for_strict(
                        cond,
                        known_i32_ranged,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                        out,
                    );
                }
                if let Some(upd) = update {
                    walk_writes_in_expr_for_strict(
                        upd,
                        known_i32_ranged,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                        out,
                    );
                }
                walk_writes_for_strict(
                    body,
                    known_i32_ranged,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                    out,
                );
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                walk_writes_for_strict(
                    body,
                    known_i32_ranged,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                    out,
                );
                if let Some(c) = catch {
                    walk_writes_for_strict(
                        &c.body,
                        known_i32_ranged,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                        out,
                    );
                }
                if let Some(f) = finally {
                    walk_writes_for_strict(
                        f,
                        known_i32_ranged,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                        out,
                    );
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                walk_writes_in_expr_for_strict(
                    discriminant,
                    known_i32_ranged,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                    out,
                );
                for c in cases {
                    if let Some(t) = &c.test {
                        walk_writes_in_expr_for_strict(
                            t,
                            known_i32_ranged,
                            flat_const_ids,
                            flat_row_alias_ids,
                            clamp_fn_ids,
                            out,
                        );
                    }
                    walk_writes_for_strict(
                        &c.body,
                        known_i32_ranged,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                        out,
                    );
                }
            }
            Stmt::Labeled { body, .. } => {
                walk_writes_for_strict(
                    std::slice::from_ref(body.as_ref()),
                    known_i32_ranged,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                    out,
                );
            }
            _ => {}
        }
    }
}

pub fn walk_writes_in_expr_for_strict(
    e: &perry_hir::Expr,
    known_i32_ranged: &HashSet<u32>,
    flat_const_ids: &HashSet<u32>,
    flat_row_alias_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
    out: &mut StrictWriteFacts,
) {
    use perry_hir::Expr;
    match e {
        Expr::LocalSet(id, value) => {
            record_strict_write(
                *id,
                value,
                known_i32_ranged,
                flat_const_ids,
                flat_row_alias_ids,
                clamp_fn_ids,
                out,
            );
            walk_writes_in_expr_for_strict(
                value,
                known_i32_ranged,
                flat_const_ids,
                flat_row_alias_ids,
                clamp_fn_ids,
                out,
            );
        }
        Expr::Update { id, .. } => {
            // #6072: `x++`/`x--` is `x = x + 1` with full f64 semantics — NOT a
            // mod-2^32 wrap (only `| 0` / `>>> 0` are ToInt32/ToUint32). An i32
            // accumulator incremented past 2^31-1 silently wraps to a negative
            // value, corrupting every later read (reads prefer the i32 shadow).
            // So an increment/decrement write is NOT strictly i32-bounded:
            // record it and disqualify the local, dropping it to the correct
            // f64 slot. Loop counters still get their i32 slot via the
            // loop-bound path, and index-used accumulators via
            // `index_used_locals` — this only removes the unsound path that let
            // a bare `let big = 2147483640; ...; big++` overflow silently.
            //
            // #6339: the disqualification now also propagates — a local that
            // *copies* `x` (`t = x`, `t = x++`) was only strict because the
            // oracle vouched for `x`, so it falls with it.
            out.saw_any.insert(*id);
            out.disqualified.insert(*id);
        }
        _ => {
            // Recurse via the centralized walker so any future Expr
            // variant carrying a `LocalSet` or `Update` is visited.
            perry_hir::walker::walk_expr_children(e, &mut |child| {
                walk_writes_in_expr_for_strict(
                    child,
                    known_i32_ranged,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                    out,
                );
            });
        }
    }
}

pub fn is_flat_const_indexget(
    e: &perry_hir::Expr,
    flat_const_ids: &HashSet<u32>,
    flat_row_alias_ids: &HashSet<u32>,
) -> bool {
    use perry_hir::Expr;
    match e {
        Expr::IndexGet { object, .. } => match object.as_ref() {
            Expr::IndexGet { object: inner, .. } => {
                matches!(inner.as_ref(), Expr::LocalGet(id) if flat_const_ids.contains(id))
            }
            Expr::LocalGet(id) => flat_row_alias_ids.contains(id),
            _ => false,
        },
        _ => false,
    }
}

/// Return `true` if `e` is a top-level bitwise Binary expression — per JS spec
/// these always produce an int32 result. Used by `collect_integer_let_ids` to
/// seed const Lets whose init is e.g. `(h >>> 16) & 0xffff` (inlined imul32
/// body variables).
/// Return `true` if `e` is the specific `(expr) >>> 0` shape — i.e. an
/// unsigned right-shift by zero, which JS uses as a u32 cast. Used to
/// gate the immutable-bitwise-init seed in `collect_integer_let_ids`
/// against u32 results that can't round-trip through a signed i32 slot.
pub fn is_ushr_zero(e: &perry_hir::Expr) -> bool {
    use perry_hir::{BinaryOp, Expr};
    matches!(
        e,
        Expr::Binary { op: BinaryOp::UShr, right, .. }
            if matches!(right.as_ref(), Expr::Integer(0))
    )
}

pub fn is_bitwise_expr(e: &perry_hir::Expr) -> bool {
    use perry_hir::{BinaryOp, Expr};
    matches!(
        e,
        Expr::Binary {
            op: BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr
                | BinaryOp::UShr,
            ..
        }
    )
}

pub fn collect_integer_let_ids(
    stmts: &[perry_hir::Stmt],
    out: &mut HashSet<u32>,
    flat_const_ids: &HashSet<u32>,
    flat_row_alias_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
) {
    use perry_hir::{Expr, Stmt};
    for s in stmts {
        match s {
            Stmt::Let {
                id,
                init: Some(init),
                mutable,
                ..
            } if matches!(init, Expr::Integer(n) if integer_literal_fits_i32(*n))
                    || is_flat_const_indexget(init, flat_const_ids, flat_row_alias_ids)
                    || is_clamp_call(init, clamp_fn_ids)
                    // Seed immutable (const) Lets whose init is a bitwise expression
                    // — EXCEPT `>>> 0`, whose result is u32 (range 0..2^32-1) and
                    // can't round-trip through a signed i32 slot. Pre-v0.5.x this
                    // wasn't a hazard because immutable lets never got an i32
                    // shadow (the `*mutable` gate kept them off); after dropping
                    // that gate (4f895dd8 — needed for `const row = yy * W` to
                    // chain through i32), `const hash = h >>> 0` would get an
                    // i32 slot and `hash.toString(16)` would print the negative
                    // form (e.g. `-2886948b` instead of `2ba2e053` on
                    // image_conv's FNV-1a checksum). Excluding the `>>> 0`
                    // shape from the seed keeps `hash` at double-only and
                    // preserves the unsigned semantics.
                    || (!mutable && is_bitwise_expr(init) && !is_ushr_zero(init))
                    // Seed mutable Lets with `(expr) | 0` init — `| 0` produces
                    // a signed 32-bit integer that fits cleanly in an i32 slot.
                    // `>>> 0` is intentionally NOT seeded here: `>>> 0` produces
                    // an UNSIGNED u32 (range 0..2^32) that doesn't round-trip
                    // through a signed i32 slot — the `LocalSet` write does
                    // `uitofp` when computing the f64 form correctly, but the
                    // i32-slot write goes through `lower_expr_as_i32` +
                    // `sitofp` and loses the high bit (e.g. `-1 >>> 0` should
                    // be 4294967295 but the i32 slot reads back as -1).
                    || (*mutable && matches!(init, Expr::Binary { op: perry_hir::BinaryOp::BitOr, right, .. } if matches!(right.as_ref(), Expr::Integer(0))))
                    // Seed mutable Lets with `init: Undefined` — the HIR
                    // lowering emits this shape for locals that get their
                    // first real value from a subsequent DoWhile or If
                    // body (the clampIdx inline expansion is the canonical
                    // case: `let xx = clampIdx(x+kx, 0, W-1)` becomes
                    // `let xx = undefined; do { ...if/else writes to
                    // xx... } while (false)`). Seed optimistically so
                    // the disqualifier's int-stable check can run on the
                    // actual writes; if any non-int write exists, the
                    // fixed-point pass removes xx from the candidate
                    // set. Without this seed, integer-valued clamp
                    // results stay double through the rest of the
                    // function — image_convolution's `idx = (row + xx)
                    // * 3` then computes in DOUBLE because xx is
                    // double, blocking i32 arithmetic on the inner
                    // kernel's address generation.
                    || (*mutable && matches!(init, Expr::Undefined)) =>
            {
                out.insert(*id);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_integer_let_ids(
                    then_branch,
                    out,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
                if let Some(eb) = else_branch {
                    collect_integer_let_ids(
                        eb,
                        out,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_integer_let_ids(
                        std::slice::from_ref(init_stmt),
                        out,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
                collect_integer_let_ids(
                    body,
                    out,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_integer_let_ids(
                    body,
                    out,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_integer_let_ids(
                    body,
                    out,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
                if let Some(c) = catch {
                    collect_integer_let_ids(
                        &c.body,
                        out,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
                if let Some(f) = finally {
                    collect_integer_let_ids(
                        f,
                        out,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    collect_integer_let_ids(
                        &c.body,
                        out,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
            }
            Stmt::Labeled { body, .. } => {
                collect_integer_let_ids(
                    std::slice::from_ref(body.as_ref()),
                    out,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
            }
            _ => {}
        }
    }
}

/// Exhaustive walker mirroring `collect_ref_ids_in_expr` but only recording
/// targets of `LocalSet`. Update (++/--) and LocalGet are intentionally NOT
/// recorded — they preserve integer-ness. Keep this in sync with
/// `collect_ref_ids_in_expr`: any new HIR Expr variant must recurse into its
/// sub-expressions here, or the walker may miss a LocalSet hidden inside it
/// and wrongly mark its target as integer-valued.
pub fn collect_localset_ids_in_stmts(stmts: &[perry_hir::Stmt], out: &mut HashSet<u32>) {
    let empty = HashSet::new();
    collect_localset_ids_in_stmts_filtered(stmts, out, None, &empty, &empty, &empty);
}

pub fn collect_localset_ids_in_stmts_filtered(
    stmts: &[perry_hir::Stmt],
    out: &mut HashSet<u32>,
    filter: Option<&HashSet<u32>>,
    flat_const_ids: &HashSet<u32>,
    flat_row_alias_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
) {
    use perry_hir::Stmt;
    for s in stmts {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) => collect_localset_ids_in_expr_filtered(
                e,
                out,
                filter,
                flat_const_ids,
                flat_row_alias_ids,
                clamp_fn_ids,
            ),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    collect_localset_ids_in_expr_filtered(
                        e,
                        out,
                        filter,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
            }
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    collect_localset_ids_in_expr_filtered(
                        e,
                        out,
                        filter,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_localset_ids_in_expr_filtered(
                    condition,
                    out,
                    filter,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
                collect_localset_ids_in_stmts_filtered(
                    then_branch,
                    out,
                    filter,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
                if let Some(eb) = else_branch {
                    collect_localset_ids_in_stmts_filtered(
                        eb,
                        out,
                        filter,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
            }
            Stmt::While { condition, body } => {
                collect_localset_ids_in_expr_filtered(
                    condition,
                    out,
                    filter,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
                collect_localset_ids_in_stmts_filtered(
                    body,
                    out,
                    filter,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
            }
            Stmt::DoWhile { body, condition } => {
                collect_localset_ids_in_stmts_filtered(
                    body,
                    out,
                    filter,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
                collect_localset_ids_in_expr_filtered(
                    condition,
                    out,
                    filter,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    collect_localset_ids_in_stmts_filtered(
                        std::slice::from_ref(init_stmt),
                        out,
                        filter,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
                if let Some(cond) = condition {
                    collect_localset_ids_in_expr_filtered(
                        cond,
                        out,
                        filter,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
                if let Some(upd) = update {
                    collect_localset_ids_in_expr_filtered(
                        upd,
                        out,
                        filter,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
                collect_localset_ids_in_stmts_filtered(
                    body,
                    out,
                    filter,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_localset_ids_in_stmts_filtered(
                    body,
                    out,
                    filter,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
                if let Some(c) = catch {
                    collect_localset_ids_in_stmts_filtered(
                        &c.body,
                        out,
                        filter,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
                if let Some(f) = finally {
                    collect_localset_ids_in_stmts_filtered(
                        f,
                        out,
                        filter,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                collect_localset_ids_in_expr_filtered(
                    discriminant,
                    out,
                    filter,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
                for c in cases {
                    if let Some(t) = &c.test {
                        collect_localset_ids_in_expr_filtered(
                            t,
                            out,
                            filter,
                            flat_const_ids,
                            flat_row_alias_ids,
                            clamp_fn_ids,
                        );
                    }
                    collect_localset_ids_in_stmts_filtered(
                        &c.body,
                        out,
                        filter,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    );
                }
            }
            Stmt::Labeled { body, .. } => {
                collect_localset_ids_in_stmts_filtered(
                    std::slice::from_ref(body.as_ref()),
                    out,
                    filter,
                    flat_const_ids,
                    flat_row_alias_ids,
                    clamp_fn_ids,
                );
            }
            _ => {}
        }
    }
}

pub fn collect_localset_ids_in_expr_filtered(
    e: &perry_hir::Expr,
    out: &mut HashSet<u32>,
    filter: Option<&HashSet<u32>>,
    flat_const_ids: &HashSet<u32>,
    flat_row_alias_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
) {
    use perry_hir::{ArrayElement, CallArg, Expr};
    let walk = |sub: &Expr, out: &mut HashSet<u32>| {
        collect_localset_ids_in_expr_filtered(
            sub,
            out,
            filter,
            flat_const_ids,
            flat_row_alias_ids,
            clamp_fn_ids,
        );
    };
    match e {
        Expr::LocalSet(id, value) => {
            match filter {
                Some(known)
                    if is_int32_producing_expr(
                        value,
                        known,
                        flat_const_ids,
                        flat_row_alias_ids,
                        clamp_fn_ids,
                    ) => {}
                _ => {
                    out.insert(*id);
                }
            }
            walk(value, out);
        }
        // Intentionally NOT recorded — these preserve integer-ness.
        Expr::LocalGet(_) | Expr::Update { .. } => {}
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            walk(left, out);
            walk(right, out);
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::Await(operand)
        | Expr::Delete(operand)
        | Expr::StringCoerce(operand)
        | Expr::ObjectCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand)
        | Expr::IsFinite(operand)
        | Expr::IsNaN(operand)
        | Expr::NumberIsNaN(operand)
        | Expr::NumberIsFinite(operand)
        | Expr::NumberIsInteger(operand)
        | Expr::IsUndefinedOrBareNan(operand)
        | Expr::ParseFloat(operand)
        | Expr::ObjectKeys(operand)
        | Expr::ForInKeys(operand)
        | Expr::ObjectValues(operand)
        | Expr::ObjectEntries(operand)
        | Expr::ObjectFromEntries(operand)
        | Expr::ObjectIsFrozen(operand)
        | Expr::ObjectIsSealed(operand)
        | Expr::ObjectIsExtensible(operand)
        | Expr::ReflectIsExtensible(operand)
        | Expr::ReflectPreventExtensions(operand)
        | Expr::SetSize(operand)
        | Expr::SetClear(operand)
        | Expr::ArrayFrom(operand)
        | Expr::ArrayFromArrayLikeHoley(operand)
        | Expr::IteratorFrom(operand)
        | Expr::Uint8ArrayFrom(operand)
        | Expr::IteratorToArray(operand)
        | Expr::GetIterator(operand)
        | Expr::GetAsyncIterator(operand)
        | Expr::ForOfToArray(operand)
        | Expr::ForAwaitToArray(operand)
        | Expr::WeakRefNew(operand)
        | Expr::WeakRefDeref(operand)
        | Expr::QueueMicrotask(operand)
        | Expr::FsExistsSync(operand)
        | Expr::FsReadFileSync(operand)
        | Expr::FsReadFileBinary(operand)
        | Expr::FsUnlinkSync(operand)
        | Expr::FsMkdirSync(operand)
        | Expr::PathDirname(operand)
        | Expr::PathBasename(operand)
        | Expr::PathExtname(operand)
        | Expr::PathResolve(operand)
        | Expr::PathNormalize(operand)
        | Expr::PathFormat(operand)
        | Expr::PathParse(operand)
        | Expr::PathToNamespacedPath(operand)
        | Expr::DateToISOString(operand)
        | Expr::DateParse(operand)
        | Expr::EnvGetDynamic(operand)
        | Expr::ErrorNew(Some(operand))
        | Expr::FinalizationRegistryNew(operand)
        | Expr::Uint8ArrayNew(Some(operand))
        | Expr::Uint8ArrayLength(operand)
        | Expr::JsonParse(operand)
        | Expr::JsonRawJson(operand)
        | Expr::JsonIsRawJson(operand)
        | Expr::MathSqrt(operand)
        | Expr::MathFloor(operand)
        | Expr::MathCeil(operand)
        | Expr::MathRound(operand)
        | Expr::MathTrunc(operand)
        | Expr::MathSign(operand)
        | Expr::MathAbs(operand)
        | Expr::MathLog(operand)
        | Expr::MathLog2(operand)
        | Expr::MathLog10(operand)
        | Expr::MathLog1p(operand)
        | Expr::MathClz32(operand)
        | Expr::MathF16round(operand)
        | Expr::MathMinSpread(operand)
        | Expr::MathMaxSpread(operand) => {
            walk(operand, out);
        }
        Expr::StructuredClone { value, options } => {
            walk(value, out);
            walk(options, out);
        }
        Expr::ObjectCreate(proto, props) => {
            walk(proto, out);
            if let Some(props) = props {
                walk(props, out);
            }
        }
        Expr::JsonParseTyped { text, .. } => walk(text, out),
        Expr::ProcessNextTick { callback, args } => {
            walk(callback, out);
            for a in args {
                walk(a, out);
            }
        }
        Expr::Call { callee, args, .. } => {
            walk(callee, out);
            for a in args {
                walk(a, out);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            walk(callee, out);
            for a in args {
                match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => walk(e, out),
                }
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                walk(o, out);
            }
            for a in args {
                walk(a, out);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            walk(condition, out);
            walk(then_expr, out);
            walk(else_expr, out);
        }
        Expr::PropertyGet { object, .. } => walk(object, out),
        Expr::PropertySet { object, value, .. } => {
            walk(object, out);
            walk(value, out);
        }
        Expr::PropertyUpdate { object, .. } => walk(object, out),
        Expr::IndexGet { object, index } => {
            walk(object, out);
            walk(index, out);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            walk(object, out);
            walk(index, out);
            walk(value, out);
        }
        Expr::ArrayPush { value, .. } => walk(value, out),
        Expr::ArraySplice {
            start,
            delete_count,
            items,
            ..
        } => {
            walk(start, out);
            if let Some(d) = delete_count {
                walk(d, out);
            }
            for it in items {
                walk(it, out);
            }
        }
        Expr::Array(elements) => {
            for el in elements {
                walk(el, out);
            }
        }
        Expr::ArraySpread(elements) => {
            for el in elements {
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => walk(e, out),
                    ArrayElement::Hole => {}
                }
            }
        }
        Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArraySort {
            array,
            comparator: callback,
        }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArrayFindLast { array, callback }
        | Expr::ArrayFindLastIndex { array, callback }
        | Expr::ArrayForEach { array, callback }
        | Expr::ArrayFlatMap { array, callback } => {
            walk(array, out);
            walk(callback, out);
        }
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        }
        | Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => {
            walk(array, out);
            walk(callback, out);
            if let Some(init) = initial {
                walk(init, out);
            }
        }
        Expr::ArrayJoin { array, separator } => {
            walk(array, out);
            if let Some(sep) = separator {
                walk(sep, out);
            }
        }
        Expr::ArraySlice { array, start, end } => {
            walk(array, out);
            walk(start, out);
            if let Some(e) = end {
                walk(e, out);
            }
        }
        Expr::ArrayIncludes {
            array,
            value,
            from_index,
        } => {
            walk(array, out);
            walk(value, out);
            if let Some(fi) = from_index {
                walk(fi, out);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                walk(v, out);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, e) in parts {
                walk(e, out);
            }
        }
        Expr::ObjectRest { object, .. } => walk(object, out),
        Expr::ObjectIs(a, b) | Expr::ObjectHasOwn(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::New { args, .. } => {
            for a in args {
                walk(a, out);
            }
        }
        Expr::MapNew | Expr::SetNew => {}
        Expr::SetNewFromArray(arr) => walk(arr, out),
        Expr::MapSet { map, key, value } => {
            walk(map, out);
            walk(key, out);
            walk(value, out);
        }
        Expr::MapGet { map, key } | Expr::MapHas { map, key } | Expr::MapDelete { map, key } => {
            walk(map, out);
            walk(key, out);
        }
        Expr::MapClear(map) => walk(map, out),
        Expr::SetAdd { value, .. } => walk(value, out),
        Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
            walk(set, out);
            walk(value, out);
        }
        Expr::MathMin(values) | Expr::MathMax(values) => {
            for v in values {
                walk(v, out);
            }
        }
        Expr::MathPow(a, b)
        | Expr::PathJoin(a, b)
        | Expr::PathRelative(a, b)
        | Expr::PathWin32Join(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::PathBasenameExt(a, b) | Expr::PathMatchesGlob(a, b) | Expr::PathResolveJoin(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::PathWin32 { args, .. } => {
            for v in args {
                walk(v, out);
            }
        }
        Expr::JsonStringifyFull(value, replacer, indent) => {
            walk(value, out);
            walk(replacer, out);
            walk(indent, out);
        }
        Expr::JsonParseReviver { text, reviver } => {
            walk(text, out);
            walk(reviver, out);
        }
        Expr::JsonParseWithReviver(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::Closure { body, .. } => {
            collect_localset_ids_in_stmts(body, out);
        }
        Expr::ParseInt { string, radix } => {
            walk(string, out);
            if let Some(r) = radix {
                walk(r, out);
            }
        }
        Expr::Sequence(es) => {
            for e in es {
                walk(e, out);
            }
        }
        Expr::InstanceOf { expr, ty_expr, .. } => {
            walk(expr, out);
            if let Some(t) = ty_expr {
                walk(t, out);
            }
        }
        Expr::In { property, object } => {
            walk(property, out);
            walk(object, out);
        }
        Expr::SuperCall(args)
        | Expr::SuperMethodCall { args, .. }
        | Expr::StaticMethodCall { args, .. } => {
            for a in args {
                walk(a, out);
            }
        }
        Expr::ObjectSuperPropertyGet {
            home,
            key,
            receiver,
        } => {
            walk(home, out);
            walk(key, out);
            walk(receiver, out);
        }
        Expr::SuperPropertySet { key, value, .. } => {
            walk(key, out);
            walk(value, out);
        }
        Expr::ObjectSuperPropertySet {
            home,
            key,
            value,
            receiver,
        } => {
            walk(home, out);
            walk(key, out);
            walk(value, out);
            walk(receiver, out);
        }
        Expr::ObjectSuperMethodCall {
            home,
            key,
            receiver,
            args,
        } => {
            walk(home, out);
            walk(key, out);
            walk(receiver, out);
            for a in args {
                walk(a, out);
            }
        }
        Expr::FsWriteFileSync(p, c) => {
            walk(p, out);
            walk(c, out);
        }
        Expr::ErrorNewWithCause { message, cause } => {
            walk(message, out);
            walk(cause, out);
        }
        Expr::ErrorNewWithOptions {
            message, options, ..
        } => {
            walk(message, out);
            walk(options, out);
        }
        Expr::DateNew(args) => {
            for a in args {
                walk(a, out);
            }
        }
        Expr::Uint8ArrayGet { array, index } => {
            walk(array, out);
            walk(index, out);
        }
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => {
            walk(array, out);
            walk(index, out);
            walk(value, out);
        }
        Expr::TypedArrayNew { arg, .. } => {
            if let Some(a) = arg {
                walk(a, out);
            }
        }
        Expr::ObjectGroupBy { items, key_fn } | Expr::MapGroupBy { items, key_fn } => {
            walk(items, out);
            walk(key_fn, out);
        }
        Expr::ArrayFromMapped {
            iterable,
            map_fn,
            this_arg,
        } => {
            walk(iterable, out);
            walk(map_fn, out);
            if let Some(t) = this_arg {
                walk(t, out);
            }
        }
        Expr::RegExpTest { regex, string } | Expr::RegExpExec { regex, string } => {
            walk(regex, out);
            walk(string, out);
        }
        Expr::StringMatch { string, regex } => {
            walk(string, out);
            walk(regex, out);
        }
        Expr::BufferFrom { data, encoding } => {
            walk(data, out);
            if let Some(e) = encoding {
                walk(e, out);
            }
        }
        Expr::BufferFromArrayBuffer {
            data,
            byte_offset,
            length,
        } => {
            walk(data, out);
            walk(byte_offset, out);
            if let Some(e) = length {
                walk(e, out);
            }
        }
        Expr::BufferAlloc {
            size,
            fill,
            encoding,
        } => {
            walk(size, out);
            if let Some(f) = fill {
                walk(f, out);
            }
            if let Some(e) = encoding {
                walk(e, out);
            }
        }
        Expr::FinalizationRegistryRegister {
            registry,
            target,
            held,
            token,
        } => {
            walk(registry, out);
            walk(target, out);
            walk(held, out);
            if let Some(t) = token {
                walk(t, out);
            }
        }
        Expr::FinalizationRegistryUnregister { registry, token } => {
            walk(registry, out);
            walk(token, out);
        }
        Expr::StaticFieldSet { value, .. } => walk(value, out),
        _ => {}
    }
}

// -------- Integer specialization for pure numeric recursive functions --------
