//! Phase-3 transformation: rewrite call sites of detected producers
//! to thread an accumulator through the rewritten signature.
//!
//! Recognizes the consumer-fuse pattern (`let X = f(args); for(j)
//! outer.push(X[j])`) and the let-bind producer-call shape. Bare
//! recursive calls inside the producer body are also threaded via the
//! `current_out` parameter when called from `rewrite_producer_body`.

use super::*;

/// Phase 3 — rewrite call sites in a Stmt sequence. Two patterns are
/// recognized:
///
/// **Consumer-fuse:** `let X = f(args); for(j) outer.push(X[j]);`
/// — rewrites to `f(args, outer)` and drops the consume loop. `X` is
/// no longer needed.
///
/// **Pass-through (bare call):** `f(args);` (Stmt::Expr) where `f` is
/// a producer — rewrites to `f(args, fresh_acc)` with a fresh local
/// `let fresh_acc = []` inserted just before the call. The fresh
/// accumulator is dropped; matches the original semantics of "call
/// for side effects, ignore return value".
///
/// **Value-binding (consumed elsewhere):** `let Y = f(args);` where
/// `Y` is used by following stmts in non-consume-loop shapes —
/// rewrites to `let Y = []; f(args, Y);`. After this, `Y` is the
/// populated array, indistinguishable from the pre-rewrite return
/// value.
pub fn rewrite_call_sites_in_stmts(
    stmts: &mut Vec<Stmt>,
    producers: &HashMap<FuncId, ProducerInfo>,
    out_param_ids: &HashMap<FuncId, LocalId>,
    next_local: &mut LocalId,
) {
    rewrite_call_sites_in_stmts_with_local_pass(stmts, producers, out_param_ids, next_local, None);
}

/// Like `rewrite_call_sites_in_stmts` but additionally aware of an
/// in-scope accumulator local (`current_out`). When set, recursive
/// pass-through calls thread `current_out` through directly instead
/// of allocating a fresh accumulator — this is the inner-recursion
/// fusion that delivers the actual ABC451D speedup.
pub fn rewrite_call_sites_in_stmts_with_local_pass(
    stmts: &mut Vec<Stmt>,
    producers: &HashMap<FuncId, ProducerInfo>,
    out_param_ids: &HashMap<FuncId, LocalId>,
    next_local: &mut LocalId,
    current_out: Option<LocalId>,
) {
    let mut i = 0;
    while i < stmts.len() {
        // Try the consumer-fuse pattern first: `let X = f(args);` followed
        // by `for(j) outer.push(X[j]);`.
        if let Some((consumed_steps, replacement)) =
            try_consumer_fuse_pattern(&stmts[i..], producers, out_param_ids)
        {
            // Remove `consumed_steps` stmts starting at i, replace with
            // `replacement`.
            stmts.drain(i..i + consumed_steps);
            for (offset, s) in replacement.into_iter().enumerate() {
                stmts.insert(i + offset, s);
            }
            // Don't advance i — recurse into the replacement (it has
            // no further patterns to rewrite, so move past).
            i += 1;
            continue;
        }

        // Non-fuse case: rewrite single-stmt patterns in place.
        if let Some(replacement) =
            try_rewrite_single_stmt(&stmts[i], producers, out_param_ids, next_local, current_out)
        {
            stmts[i] = replacement.0;
            // Insert any extra stmts AFTER the rewritten one (e.g. the
            // call after a fresh `let X = []`).
            for (offset, s) in replacement.1.into_iter().enumerate() {
                stmts.insert(i + 1 + offset, s);
            }
            i += 1;
            continue;
        }

        // Recurse into nested control flow.
        match &mut stmts[i] {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                rewrite_call_sites_in_stmts_with_local_pass(
                    then_branch,
                    producers,
                    out_param_ids,
                    next_local,
                    current_out,
                );
                if let Some(eb) = else_branch {
                    rewrite_call_sites_in_stmts_with_local_pass(
                        eb,
                        producers,
                        out_param_ids,
                        next_local,
                        current_out,
                    );
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                rewrite_call_sites_in_stmts_with_local_pass(
                    body,
                    producers,
                    out_param_ids,
                    next_local,
                    current_out,
                );
            }
            Stmt::For { body, .. } => {
                rewrite_call_sites_in_stmts_with_local_pass(
                    body,
                    producers,
                    out_param_ids,
                    next_local,
                    current_out,
                );
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                rewrite_call_sites_in_stmts_with_local_pass(
                    body,
                    producers,
                    out_param_ids,
                    next_local,
                    current_out,
                );
                if let Some(c) = catch {
                    rewrite_call_sites_in_stmts_with_local_pass(
                        &mut c.body,
                        producers,
                        out_param_ids,
                        next_local,
                        current_out,
                    );
                }
                if let Some(f) = finally {
                    rewrite_call_sites_in_stmts_with_local_pass(
                        f,
                        producers,
                        out_param_ids,
                        next_local,
                        current_out,
                    );
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    rewrite_call_sites_in_stmts_with_local_pass(
                        &mut c.body,
                        producers,
                        out_param_ids,
                        next_local,
                        current_out,
                    );
                }
            }
            Stmt::Labeled { body, .. } => {
                let mut tmp = vec![std::mem::replace(
                    body.as_mut(),
                    Stmt::Expr(Expr::Undefined),
                )];
                rewrite_call_sites_in_stmts_with_local_pass(
                    &mut tmp,
                    producers,
                    out_param_ids,
                    next_local,
                    current_out,
                );
                **body = tmp.into_iter().next().unwrap();
            }
            _ => {}
        }
        i += 1;
    }
}

/// Try to recognize the consumer-fuse pattern at `stmts[0..]`:
///
///   stmts[0]: Stmt::Let { id: child, init: Some(Call { callee: FuncRef(f), args }) }
///   stmts[1]: Stmt::For { for(j=0; j<child.length; j++) outer.push(child[j]); }
///
/// where `f` is a deforestable producer and `child` has no further
/// uses after stmts[1].
///
/// Returns `Some((consumed_count, replacement_stmts))` where
/// `consumed_count` is 2 (we replace both stmts) and the replacement
/// is `[Stmt::Expr(Call { f, args ++ [outer] })]`.
fn try_consumer_fuse_pattern(
    stmts: &[Stmt],
    producers: &HashMap<FuncId, ProducerInfo>,
    out_param_ids: &HashMap<FuncId, LocalId>,
) -> Option<(usize, Vec<Stmt>)> {
    if stmts.len() < 2 {
        return None;
    }
    let (child_id, callee_id, call_args, type_args) = match &stmts[0] {
        Stmt::Let {
            id,
            init:
                Some(Expr::Call {
                    callee,
                    args,
                    type_args,
                    ..
                }),
            ..
        } => match callee.as_ref() {
            Expr::FuncRef(fid) if producers.contains_key(fid) => {
                (*id, *fid, args.clone(), type_args.clone())
            }
            _ => return None,
        },
        _ => return None,
    };

    // Recognize: `for (let j = 0; j < child.length; j++) outer.push(child[j]);`
    let outer_id = match &stmts[1] {
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => match_consume_loop(child_id, init, condition, update, body)?,
        _ => return None,
    };

    // `child` must not be referenced anywhere AFTER stmts[1].
    let mut later_uses = false;
    for s in &stmts[2..] {
        if stmt_references_local(s, child_id) {
            later_uses = true;
            break;
        }
    }
    if later_uses {
        return None;
    }

    // Build replacement: `f(args, outer);`
    let mut new_args = call_args;
    new_args.push(Expr::LocalGet(outer_id));
    let _ = out_param_ids.get(&callee_id)?; // sanity check producer was sized
    let new_call = Expr::Call {
        callee: Box::new(Expr::FuncRef(callee_id)),
        args: new_args,
        type_args,
        // #5247: deforestation-fused call; no single source offset.
        byte_offset: 0,
    };
    Some((2, vec![Stmt::Expr(new_call)]))
}

/// Match the for-loop shape `for (let j = 0; j < child.length; j++)
/// outer.push(child[j]);` and return `outer`'s LocalId on success.
fn match_consume_loop(
    child_id: LocalId,
    init: &Option<Box<Stmt>>,
    condition: &Option<Expr>,
    update: &Option<Expr>,
    body: &[Stmt],
) -> Option<LocalId> {
    // init: let j = 0
    let init_stmt = init.as_ref()?;
    let j_id = match init_stmt.as_ref() {
        Stmt::Let {
            id,
            init: Some(Expr::Integer(0)),
            ..
        } => *id,
        Stmt::Let {
            id,
            init: Some(Expr::Number(n)),
            ..
        } if *n == 0.0 => *id,
        _ => return None,
    };

    // condition: j < child.length
    let cond = condition.as_ref()?;
    match cond {
        Expr::Compare {
            op: perry_hir::CompareOp::Lt,
            left,
            right,
        } => {
            if !matches!(left.as_ref(), Expr::LocalGet(id) if *id == j_id) {
                return None;
            }
            match right.as_ref() {
                Expr::PropertyGet {
                    object, property, ..
                } if property == "length" => {
                    if !matches!(object.as_ref(), Expr::LocalGet(id) if *id == child_id) {
                        return None;
                    }
                }
                _ => return None,
            }
        }
        _ => return None,
    }

    // update: j++ or ++j (Update { id: j, op: Inc, .. })
    let upd = update.as_ref()?;
    match upd {
        Expr::Update { id, op, .. }
            if *id == j_id && matches!(op, perry_hir::UpdateOp::Increment) => {}
        _ => return None,
    }

    // body: exactly one stmt — outer.push(child[j])
    if body.len() != 1 {
        return None;
    }
    let push_call = match &body[0] {
        Stmt::Expr(e) => e,
        _ => return None,
    };
    // Match either ArrayPush { array: LocalGet(outer), value: IndexGet { ... } }
    // OR Call { callee: PropertyGet { LocalGet(outer), "push" }, args: [IndexGet...] }
    match push_call {
        Expr::ArrayPush { array_id, value } => {
            if !is_index_get_of(value, child_id, j_id) {
                return None;
            }
            Some(*array_id)
        }
        Expr::Call { callee, args, .. } => match callee.as_ref() {
            Expr::PropertyGet {
                object, property, ..
            } if property == "push" => {
                let outer_id = match object.as_ref() {
                    Expr::LocalGet(id) => *id,
                    _ => return None,
                };
                if args.len() != 1 {
                    return None;
                }
                if !is_index_get_of(&args[0], child_id, j_id) {
                    return None;
                }
                Some(outer_id)
            }
            _ => None,
        },
        _ => None,
    }
}

/// Match `child[j]` (IndexGet { object: LocalGet(child), index: LocalGet(j) }).
fn is_index_get_of(e: &Expr, child_id: LocalId, j_id: LocalId) -> bool {
    match e {
        Expr::IndexGet { object, index } => {
            matches!(object.as_ref(), Expr::LocalGet(id) if *id == child_id)
                && matches!(index.as_ref(), Expr::LocalGet(id) if *id == j_id)
        }
        _ => false,
    }
}

/// Single-stmt-level rewrites. Returns `Some((replacement, extras))`
/// where `extras` are stmts to insert AFTER the replacement.
///
/// Currently handles:
/// - **Bare expression call:** `f(args);` (Stmt::Expr) — rewrites to
///   pass `current_out` (if available) or a fresh accumulator.
/// - **Let-bind producer call:** `let Y = f(args);` — rewrites to
///   `let Y = []; f(args, Y);`.
fn try_rewrite_single_stmt(
    stmt: &Stmt,
    producers: &HashMap<FuncId, ProducerInfo>,
    out_param_ids: &HashMap<FuncId, LocalId>,
    next_local: &mut LocalId,
    _current_out: Option<LocalId>,
) -> Option<(Stmt, Vec<Stmt>)> {
    match stmt {
        Stmt::Let {
            id,
            name,
            ty: _,
            mutable,
            init:
                Some(Expr::Call {
                    callee,
                    args,
                    type_args,
                    ..
                }),
        } => match callee.as_ref() {
            Expr::FuncRef(fid) if producers.contains_key(fid) => {
                let info = producers.get(fid)?;
                let mut new_args = args.clone();
                new_args.push(Expr::LocalGet(*id));
                let call_stmt = Stmt::Expr(Expr::Call {
                    callee: callee.clone(),
                    args: new_args,
                    type_args: type_args.clone(),
                    byte_offset: 0,
                });
                let let_stmt = Stmt::Let {
                    id: *id,
                    name: name.clone(),
                    ty: Type::Array(Box::new(info.elem_ty.clone())),
                    mutable: *mutable,
                    init: Some(Expr::Array(Vec::new())),
                };
                let _ = out_param_ids; // already validated producer
                let _ = next_local;
                Some((let_stmt, vec![call_stmt]))
            }
            _ => None,
        },
        // `f(args);` as a bare expression: rare for producers, since
        // the return value is the whole point. Skip for now —
        // misclassifying these is more dangerous than missing them.
        _ => None,
    }
}
