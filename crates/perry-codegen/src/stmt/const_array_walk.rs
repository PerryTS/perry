//! `for (const x of a)` over an immutable Array (wpfacts fact IM,
//! `perry_wpfacts` `const_arrays`; CENSUS-p4 §5 lever 6, CENSUS-residual N3).
//!
//! The HIR lowers the loop as (`perry_hir` `for_of_guard`, `source_for_of`)
//!
//! ```text
//! const t = a;
//! if (arrayRecordNeedsIterator(t, true) == false) {
//!   for (let k = 0; k < t.length; k++) { const x = t[k]; body }
//! } else {
//!   for (const x of t) body            // the IteratorRecord loop
//! }
//! ```
//!
//! and the entry is a header test of `t` per loop, with the runtime behind
//! it. When the facts give `t` the representation `Arr` with `im` (every
//! Array it may hold is a plain program Array nothing writes after its
//! birth), and no program write can reach an iteration member or define an
//! element descriptor (`array_program_facts` is 0), the entry is decided
//! here: the walk is taken, the IteratorRecord loop is not lowered.
//!
//! **Constant contents** (`ElisionFacts::const_arrays`): every Array `t` may
//! hold has the same Number elements `c0 … cn-1` for the whole program. The
//! walk reads `length`, then the element, at each step; both are constants,
//! so the loop is its `n` iterations in order, each with `x` bound to its
//! Number literal:
//!
//! ```text
//! { const x = c0; body }  …  { const x = cn-1; body }
//! ```
//!
//! `continue` goes to the next copy, `break` past the last one. No counter,
//! no element read and no back edge remain, so no loop poll either. The
//! copies are bounded: at most `MAX_UNROLL` elements, and `n` times the
//! body's HIR size (`body_cost`, which counts a nested walk at its largest
//! unrolling) at most `MAX_UNROLLED_COST`. A loop over the bound keeps the
//! counted walk.
//!
//! **Any other immutable receiver** (several literals with different
//! contents: `c ? BISHOP : ROOK`): the counted walk over `t`. Its packed-f64
//! admission is true when the elements are Numbers below a dense floor (an
//! immutable Array's slots are its birth's plain doubles, its length never
//! moves), else the walk reads through the element tier.
//!
//! Check mode (`PERRY_VERIFY_FACTS=1`): the entry the proof replaces is run
//! (`js_array_record_enter_counted`) and must give the verdict taken, and
//! each constant is compared with the slot's bits (`Fact::ConstArray`,
//! exit 86).

use perry_hir::{CompareOp, Expr, Stmt, UpdateOp};

use super::*;
use crate::codegen::{FactsArrRep, FactsElemRep, FactsRep};
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{I1, I32, I64, I8, PTR};

/// The most elements an unrolled walk has.
const MAX_UNROLL: usize = 8;
/// The most HIR nodes the copies of an unrolled walk hold together.
const MAX_UNROLLED_COST: usize = 1024;

/// The walk of `source_for_of`, as the `if` holds it.
struct Walk<'a> {
    /// The source temp `t`.
    source: u32,
    /// The statements before the loop (`const a: T[] = t`, or none).
    prelude: &'a [Stmt],
    /// The loop binding's `let`, `const x = t[k]`.
    binding: &'a Stmt,
    /// The loop body after the binding.
    body: &'a [Stmt],
    /// The whole counted loop body, binding first.
    loop_body: &'a [Stmt],
    /// The entry asked for the packed-f64 admission (`t` is a Number array).
    counted: bool,
}

/// `cond` is the walk's entry over a local: that local and whether the entry
/// is the counted one.
fn entry(cond: &Expr) -> Option<(u32, bool)> {
    let Expr::Compare {
        op: CompareOp::Eq,
        left,
        right,
    } = cond
    else {
        return None;
    };
    if !matches!(right.as_ref(), Expr::Bool(false)) {
        return None;
    }
    let Expr::NativeMethodCall {
        module,
        class_name: None,
        object: None,
        method,
        args,
    } = left.as_ref()
    else {
        return None;
    };
    if module != "__perry_runtime" || method != "arrayRecordNeedsIterator" {
        return None;
    }
    match args.as_slice() {
        [Expr::LocalGet(id)] => Some((*id, false)),
        [Expr::LocalGet(id), Expr::Bool(true)] => Some((*id, true)),
        _ => None,
    }
}

fn walk<'a>(cond: &Expr, then_branch: &'a [Stmt]) -> Option<Walk<'a>> {
    let (source, counted) = entry(cond)?;
    let (prelude, lp) = then_branch.split_at(then_branch.len().checked_sub(1)?);
    let array = match prelude {
        [] => source,
        [Stmt::Let {
            id,
            mutable: false,
            init: Some(Expr::LocalGet(from)),
            ..
        }] if *from == source => *id,
        _ => return None,
    };
    let Stmt::For {
        init: Some(init),
        condition: Some(Expr::Compare {
            op: CompareOp::Lt,
            left,
            right,
        }),
        update: Some(Expr::Update {
            id: stepped,
            op: UpdateOp::Increment,
            prefix: false,
        }),
        body: loop_body,
    } = &lp[0]
    else {
        return None;
    };
    let Stmt::Let {
        id: counter,
        init: Some(Expr::Integer(0)),
        ..
    } = init.as_ref()
    else {
        return None;
    };
    let length_of_array = matches!(
        right.as_ref(),
        Expr::PropertyGet { object, property, .. }
            if property == "length" && matches!(object.as_ref(), Expr::LocalGet(a) if *a == array)
    );
    if stepped != counter
        || !matches!(left.as_ref(), Expr::LocalGet(k) if k == counter)
        || !length_of_array
    {
        return None;
    }
    let (binding, body) = loop_body.split_first()?;
    let Stmt::Let {
        init: Some(Expr::IndexGet { object, index, .. }),
        ..
    } = binding
    else {
        return None;
    };
    if !matches!(object.as_ref(), Expr::LocalGet(a) if *a == array)
        || !matches!(index.as_ref(), Expr::LocalGet(k) if k == counter)
    {
        return None;
    }
    // The counter is the walk's own: the body cannot name it.
    Some(Walk {
        source,
        prelude,
        binding,
        body,
        loop_body,
        counted,
    })
}

/// HIR nodes of `stmts`: each statement and each expression, a nested walk
/// at `MAX_UNROLL` copies of its loop (what unrolling it could emit).
fn body_cost(stmts: &[Stmt]) -> usize {
    fn expr_cost(e: &Expr) -> usize {
        let mut n = 1;
        perry_hir::walker::walk_expr_children(e, &mut |c| n += expr_cost(c));
        n
    }
    fn opt(e: &Option<Expr>) -> usize {
        e.as_ref().map_or(0, expr_cost)
    }
    fn stmt_cost(s: &Stmt) -> usize {
        1 + match s {
            Stmt::Let { init, .. } | Stmt::Return(init) => opt(init),
            Stmt::Expr(e) | Stmt::Throw(e) => expr_cost(e),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let nested = if walk(condition, then_branch).is_some() {
                    MAX_UNROLL
                } else {
                    1
                };
                expr_cost(condition)
                    + nested * body_cost(then_branch)
                    + else_branch.as_ref().map_or(0, |b| body_cost(b))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                expr_cost(condition) + body_cost(body)
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().map_or(0, |s| stmt_cost(s))
                    + opt(condition)
                    + opt(update)
                    + body_cost(body)
            }
            Stmt::Labeled { body, .. } => stmt_cost(body),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                body_cost(body)
                    + catch.as_ref().map_or(0, |c| body_cost(&c.body))
                    + finally.as_ref().map_or(0, |b| body_cost(b))
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                expr_cost(discriminant)
                    + cases
                        .iter()
                        .map(|c| opt(&c.test) + body_cost(&c.body))
                        .sum::<usize>()
            }
            // Any other statement is a leaf (`break`, `continue`, a label
            // jump) or holds what the variants above do not name: counted
            // as one node, as a leaf is.
            _ => 0,
        }
    }
    stmts.iter().map(stmt_cost).sum()
}

/// The Number with bits `bits` as the literal the HIR has for it.
fn literal(bits: u64) -> Expr {
    let v = f64::from_bits(bits);
    let int = v as i64;
    if v.is_finite() && (int as f64).to_bits() == bits && int.unsigned_abs() < (1 << 53) {
        Expr::Integer(int)
    } else {
        Expr::Number(v)
    }
}

/// Lower `if (cond) then_branch else …` when it is the walk of a for-of
/// over an immutable Array (module docs). `false`: not such a walk; the
/// caller lowers the `if`.
pub(super) fn lower(ctx: &mut FnCtx<'_>, cond: &Expr, then_branch: &[Stmt]) -> Result<bool> {
    if ctx.wpfacts_off || crate::expr::typed_feedback_emission_enabled() {
        return Ok(false);
    }
    let Some(w) = walk(cond, then_branch) else {
        return Ok(false);
    };
    // A local the codegen keeps in scalar slots holds no array word.
    if ctx.scalar_replaced_arrays.contains_key(&w.source)
        || ctx.array_row_aliases.contains_key(&w.source)
        || ctx.scalar_replaced.contains_key(&w.source)
    {
        return Ok(false);
    }
    let Some(FactsRep::Arr(rep)) = crate::codegen::facts_rep::local(ctx, w.source) else {
        return Ok(false);
    };
    if !rep.im {
        return Ok(false);
    }
    if crate::expr::array_program_facts(&ctx.wpfacts) != 0 {
        return Ok(false);
    }
    let consts = ctx
        .wpfacts
        .const_arrays
        .get(&w.source)
        .filter(|c| {
            c.len() <= MAX_UNROLL
                && c.len() as u64 == u64::from(rep.floor)
                && c.len() * (1 + body_cost(w.body)) <= MAX_UNROLLED_COST
        })
        .cloned();
    let packed = w.counted && rep.dense && rep.floor > 0 && rep.elem != FactsElemRep::Boxed;
    crate::verify_facts::covered(crate::verify_facts::Fact::ConstArray, || {
        serde_json::json!({
            "local": w.source,
            "unrolled": consts.as_ref().map(|c| c.len()),
            "packed": packed,
        })
    });
    witness(ctx, &w, rep, consts.as_deref(), packed)?;
    match consts {
        Some(c) => unrolled(ctx, &w, &c),
        None => {
            if w.counted {
                // The counted loop over `t` consumes its admission
                // (`record_packed_admissions`) in place of a guard call.
                let slot = ctx.func.alloca_entry(I1);
                ctx.block()
                    .store(I1, if packed { "true" } else { "false" }, &slot);
                ctx.record_packed_admissions.insert(w.source, slot);
            }
            lower_stmts(ctx, w.prelude)?;
            let lp = &then_branch[w.prelude.len()..];
            lower_stmts(ctx, lp)?;
            let _ = w.loop_body;
            Ok(())
        }
    }?;
    Ok(true)
}

/// The `n` iterations in order (module docs).
fn unrolled(ctx: &mut FnCtx<'_>, w: &Walk<'_>, consts: &[u64]) -> Result<()> {
    let Stmt::Let {
        id,
        name,
        ty,
        mutable,
        ..
    } = w.binding
    else {
        unreachable!("walk() matched the binding");
    };
    // A label pending on the loop names no jump in the body (`for_of_guard`
    // refuses a labeled jump there); it is consumed as the loop would.
    let _ = super::take_loop_labels(ctx, w.loop_body);
    let exit = ctx.new_block("forof.im.exit");
    let exit_label = ctx.block_label(exit);
    for (i, bits) in consts.iter().enumerate() {
        let next = if i + 1 < consts.len() {
            ctx.new_block("forof.im.next")
        } else {
            exit
        };
        let next_label = ctx.block_label(next);
        ctx.loop_targets
            .push((next_label.clone(), exit_label.clone(), ctx.try_depth));
        let bound = Stmt::Let {
            id: *id,
            name: name.clone(),
            ty: ty.clone(),
            mutable: *mutable,
            init: Some(literal(*bits)),
        };
        let lowered = lower_stmt(ctx, &bound).and_then(|()| lower_stmts(ctx, w.body));
        if lowered.is_ok() {
            loops::clear_loop_body_shadow_slots(ctx, w.loop_body);
        }
        ctx.loop_targets.pop();
        lowered?;
        if !ctx.block().is_terminated() {
            ctx.block().br(&next_label);
        }
        ctx.current_block = next;
    }
    if consts.is_empty() {
        ctx.block().br(&exit_label);
        ctx.current_block = exit;
    }
    Ok(())
}

/// Check mode: the entry's own verdict on `t`, and the constants.
fn witness(
    ctx: &mut FnCtx<'_>,
    w: &Walk<'_>,
    rep: FactsArrRep,
    consts: Option<&[u64]>,
    packed: bool,
) -> Result<()> {
    if !crate::verify_facts::enabled() {
        return Ok(());
    }
    let recv = lower_expr(ctx, &Expr::LocalGet(w.source))?;
    let miss = ctx.new_block("forof.im.verify_miss");
    let next = ctx.new_block("forof.im.admitted");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    crate::verify_facts::tick(ctx);
    // The entry this proof replaces: no iterator protocol (bit 0 clear),
    // the same head, and the packed-f64 admission when it is taken.
    let slot = ctx.func.alloca_entry(DOUBLE);
    let verdict = ctx.block().call(
        I32,
        "js_array_record_enter_counted",
        &[(DOUBLE, &recv), (PTR, &slot), (I32, "0")],
    );
    let mask = if packed { "3" } else { "1" };
    let want = if packed { "2" } else { "0" };
    let bits = ctx.block().and(I32, &verdict, mask);
    let agreed = ctx.block().icmp_eq(I32, &bits, want);
    let head = ctx.block().load(DOUBLE, &slot);
    let head_bits = ctx.block().bitcast_double_to_i64(&head);
    let recv_bits = ctx.block().bitcast_double_to_i64(&recv);
    let same = ctx.block().icmp_eq(I64, &head_bits, &recv_bits);
    let ok = ctx.block().and(I1, &agreed, &same);
    branch(ctx, &ok, "forof.im.verify_length", &miss_l);
    // Immutable: the length is the floor, for good.
    let handle = ctx.block().and(I64, &recv_bits, POINTER_MASK_I64);
    let ptr = ctx.block().inttoptr(I64, &handle);
    let length = ctx.block().load(I32, &ptr);
    let fixed = ctx.block().icmp_eq(I32, &length, &rep.floor.to_string());
    if let Some(consts) = consts {
        branch(ctx, &fixed, "forof.im.verify_elems", &miss_l);
        let cap_ptr = ctx.block().gep(I8, &ptr, &[(I64, "4")]);
        let capacity = ctx.block().load(I32, &cap_ptr);
        for (i, c) in consts.iter().enumerate() {
            let addr =
                ctx.block()
                    .array_element_addr_with_capacity(&handle, &capacity, &i.to_string());
            let at = ctx.block().inttoptr(I64, &addr);
            let raw = ctx.block().load(I64, &at);
            let equal = ctx.block().icmp_eq(I64, &raw, &(*c as i64).to_string());
            if i + 1 < consts.len() {
                branch(ctx, &equal, "forof.im.verify_elem", &miss_l);
            } else {
                ctx.block().cond_br(&equal, &next_l, &miss_l);
            }
        }
        if consts.is_empty() {
            ctx.block().br(&next_l);
        }
    } else {
        ctx.block().cond_br(&fixed, &next_l, &miss_l);
    }
    ctx.current_block = miss;
    let site = crate::verify_facts::site_arg(
        ctx,
        &format!(
            "constant array of local {}: floor {}, elements {:?}",
            w.source,
            rep.floor,
            consts.map(|c| c.iter().map(|b| f64::from_bits(*b)).collect::<Vec<_>>())
        ),
        crate::verify_facts::Fact::ConstArray,
        &[],
    );
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, &recv)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
    Ok(())
}

/// `cond` goes on to a new block `name`, else to `other`; the new block is
/// current.
fn branch(ctx: &mut FnCtx<'_>, cond: &str, name: &str, other: &str) {
    let on = ctx.new_block(name);
    let on_l = ctx.block_label(on);
    ctx.block().cond_br(cond, &on_l, other);
    ctx.current_block = on;
}
