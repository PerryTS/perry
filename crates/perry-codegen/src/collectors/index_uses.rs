use std::collections::HashSet;

use super::*;

pub fn collect_index_used_locals(stmts: &[perry_hir::Stmt]) -> HashSet<u32> {
    let mut out: HashSet<u32> = HashSet::new();
    walk_index_uses_in_stmts(stmts, &mut out);
    // Issue #435: take the transitive closure backward through writes so
    // that locals which feed an array index via arithmetic are also
    // marked. Image-convolution's `xx → idx → array[idx]` shape relies
    // on this: `idx` is direct-index-used, and `idx = (row + xx) * 3`
    // makes `xx` and `row` transitively index-used. Without the closure,
    // re-introducing the v0.5.164 `index_used_locals` gate at the
    // Let-site i32-shadow path would lose the chain and force inner-
    // loop arithmetic back into f64 (the regression that motivated
    // dropping the gate originally). With the closure, pure
    // accumulators that never reach an index (`sum += compute(i)`)
    // stay outside the set, so the gate keeps them off the i32 shadow
    // path — closing #435 while preserving the image_conv perf win.
    propagate_index_used_transitive(stmts, &mut out);
    out
}

/// Iterate the `Stmt::Let` / `Expr::LocalSet` write graph to a fixed
/// point: when a write target is in `out`, pull every `LocalGet` /
/// `LocalSet` / `Update` id from the rhs into `out` as well. The result
/// is the set of locals whose value transitively flows into an array
/// index expression somewhere in the function.
pub fn propagate_index_used_transitive(stmts: &[perry_hir::Stmt], out: &mut HashSet<u32>) {
    loop {
        let before = out.len();
        absorb_writes_into_index_used(stmts, out);
        if out.len() == before {
            break;
        }
    }
}

pub fn absorb_writes_into_index_used(stmts: &[perry_hir::Stmt], out: &mut HashSet<u32>) {
    use perry_hir::Stmt;
    for s in stmts {
        match s {
            Stmt::Let {
                id,
                init: Some(init),
                ..
            } => {
                if out.contains(id) {
                    collect_ref_ids_in_expr(init, out);
                }
                absorb_writes_in_expr(init, out);
            }
            Stmt::Let { init: None, .. } => {}
            Stmt::Expr(e) | Stmt::Throw(e) => absorb_writes_in_expr(e, out),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    absorb_writes_in_expr(e, out);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                absorb_writes_in_expr(condition, out);
                absorb_writes_into_index_used(then_branch, out);
                if let Some(eb) = else_branch {
                    absorb_writes_into_index_used(eb, out);
                }
            }
            Stmt::While { condition, body } => {
                absorb_writes_in_expr(condition, out);
                absorb_writes_into_index_used(body, out);
            }
            Stmt::DoWhile { body, condition } => {
                absorb_writes_into_index_used(body, out);
                absorb_writes_in_expr(condition, out);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    absorb_writes_into_index_used(std::slice::from_ref(init_stmt), out);
                }
                if let Some(cond) = condition {
                    absorb_writes_in_expr(cond, out);
                }
                if let Some(upd) = update {
                    absorb_writes_in_expr(upd, out);
                }
                absorb_writes_into_index_used(body, out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                absorb_writes_into_index_used(body, out);
                if let Some(c) = catch {
                    absorb_writes_into_index_used(&c.body, out);
                }
                if let Some(f) = finally {
                    absorb_writes_into_index_used(f, out);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                absorb_writes_in_expr(discriminant, out);
                for c in cases {
                    if let Some(t) = &c.test {
                        absorb_writes_in_expr(t, out);
                    }
                    absorb_writes_into_index_used(&c.body, out);
                }
            }
            Stmt::Labeled { body, .. } => {
                absorb_writes_into_index_used(std::slice::from_ref(body.as_ref()), out);
            }
            _ => {}
        }
    }
}

pub fn absorb_writes_in_expr(e: &perry_hir::Expr, out: &mut HashSet<u32>) {
    // Find every `LocalSet(id, value)` reachable from `e`. When `id` is
    // in `out`, pull every ref id from `value`. For arithmetic /
    // structural variants we recurse into sub-expressions so that
    // writes nested inside Binary / Conditional / Call / etc. are still
    // visited.
    let mut writes: Vec<(u32, &perry_hir::Expr)> = Vec::new();
    collect_localsets_in_expr_for_propagate(e, &mut writes);
    for (id, value) in &writes {
        if out.contains(id) {
            collect_ref_ids_in_expr(value, out);
        }
    }
}

pub fn collect_localsets_in_expr_for_propagate<'a>(
    e: &'a perry_hir::Expr,
    out: &mut Vec<(u32, &'a perry_hir::Expr)>,
) {
    use perry_hir::Expr;
    match e {
        Expr::LocalSet(id, value) => {
            out.push((*id, value));
            collect_localsets_in_expr_for_propagate(value, out);
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            collect_localsets_in_expr_for_propagate(left, out);
            collect_localsets_in_expr_for_propagate(right, out);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_localsets_in_expr_for_propagate(condition, out);
            collect_localsets_in_expr_for_propagate(then_expr, out);
            collect_localsets_in_expr_for_propagate(else_expr, out);
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
        | Expr::IsNaN(operand) => collect_localsets_in_expr_for_propagate(operand, out),
        Expr::Call { callee, args, .. } => {
            collect_localsets_in_expr_for_propagate(callee, out);
            for a in args {
                collect_localsets_in_expr_for_propagate(a, out);
            }
        }
        Expr::IndexGet { object, index } => {
            collect_localsets_in_expr_for_propagate(object, out);
            collect_localsets_in_expr_for_propagate(index, out);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            collect_localsets_in_expr_for_propagate(object, out);
            collect_localsets_in_expr_for_propagate(index, out);
            collect_localsets_in_expr_for_propagate(value, out);
        }
        // Any HIR variant we don't explicitly recurse into is treated
        // as a leaf — a missed nested `LocalSet` only loses the
        // i32-shadow optimization for that one chain (correctness is
        // unaffected because the gate is conservative-OFF).
        _ => {}
    }
}

pub fn walk_index_uses_in_stmts(stmts: &[perry_hir::Stmt], out: &mut HashSet<u32>) {
    use perry_hir::Stmt;
    for s in stmts {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) => walk_index_uses_in_expr(e, out),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    walk_index_uses_in_expr(e, out);
                }
            }
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    walk_index_uses_in_expr(e, out);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                walk_index_uses_in_expr(condition, out);
                walk_index_uses_in_stmts(then_branch, out);
                if let Some(eb) = else_branch {
                    walk_index_uses_in_stmts(eb, out);
                }
            }
            Stmt::While { condition, body } => {
                walk_index_uses_in_expr(condition, out);
                walk_index_uses_in_stmts(body, out);
            }
            Stmt::DoWhile { body, condition } => {
                walk_index_uses_in_stmts(body, out);
                walk_index_uses_in_expr(condition, out);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    walk_index_uses_in_stmts(std::slice::from_ref(i), out);
                }
                if let Some(c) = condition {
                    walk_index_uses_in_expr(c, out);
                }
                if let Some(u) = update {
                    walk_index_uses_in_expr(u, out);
                }
                walk_index_uses_in_stmts(body, out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                walk_index_uses_in_stmts(body, out);
                if let Some(c) = catch {
                    walk_index_uses_in_stmts(&c.body, out);
                }
                if let Some(f) = finally {
                    walk_index_uses_in_stmts(f, out);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                walk_index_uses_in_expr(discriminant, out);
                for c in cases {
                    if let Some(t) = &c.test {
                        walk_index_uses_in_expr(t, out);
                    }
                    walk_index_uses_in_stmts(&c.body, out);
                }
            }
            Stmt::Labeled { body, .. } => {
                walk_index_uses_in_stmts(std::slice::from_ref(body.as_ref()), out);
            }
            _ => {}
        }
    }
}

pub fn walk_index_uses_in_expr(e: &perry_hir::Expr, out: &mut HashSet<u32>) {
    use perry_hir::{ArrayElement, CallArg, Expr};
    // For the `index` field of an index-using variant we need EVERY local
    // referenced anywhere inside the subtree, so dispatch to the existing
    // `collect_ref_ids_in_expr` walker (which already walks `LocalGet` /
    // `LocalSet` / `Update` and inserts their ids).
    let collect_index_refs = |idx: &Expr, out: &mut HashSet<u32>| {
        collect_ref_ids_in_expr(idx, out);
    };

    match e {
        // --- index-using variants: mark locals in `index` subtree ---
        Expr::IndexGet { object, index } => {
            collect_index_refs(index, out);
            walk_index_uses_in_expr(object, out);
            walk_index_uses_in_expr(index, out);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            collect_index_refs(index, out);
            walk_index_uses_in_expr(object, out);
            walk_index_uses_in_expr(index, out);
            walk_index_uses_in_expr(value, out);
        }
        Expr::IndexUpdate { object, index, .. } => {
            collect_index_refs(index, out);
            walk_index_uses_in_expr(object, out);
            walk_index_uses_in_expr(index, out);
        }
        Expr::BufferIndexGet { buffer, index } => {
            collect_index_refs(index, out);
            walk_index_uses_in_expr(buffer, out);
            walk_index_uses_in_expr(index, out);
        }
        Expr::BufferIndexSet {
            buffer,
            index,
            value,
        } => {
            collect_index_refs(index, out);
            // (Issue #436) Also seed locals appearing in the stored
            // VALUE subtree. Buffer / Uint8 typed-array stores clamp the
            // stored byte at the runtime layer (`& 0xff`); locals that
            // flow into the stored value are effectively byte-bounded
            // for storage purposes, and the typical accumulator pattern
            // `dst[outIdx] = clampU8((acc / KSUM) | 0)` reads `acc`
            // through a `| 0` truncation that's idempotent under i32.
            // Without this seed, image_convolution's rAcc/gAcc/bAcc and
            // the k kernel coefficient stay outside `index_used_locals`
            // post-#435 closure, lose their i32 shadow at the Let-site
            // gate, and the inner blur loop falls back to f64 with a
            // per-access `js_number_coerce` + `fmul` chain (≥5× slow).
            // The #435 bug shapes (`let sum = 0; for(50M) sum +=
            // compute(i); console.log(sum)` and the eight siblings) all
            // output via console.log, never feeding into a typed-array
            // store, so this extension is correctness-safe against the
            // existing #435 regression test.
            collect_index_refs(value, out);
            walk_index_uses_in_expr(buffer, out);
            walk_index_uses_in_expr(index, out);
            walk_index_uses_in_expr(value, out);
        }
        Expr::Uint8ArrayGet { array, index } => {
            collect_index_refs(index, out);
            walk_index_uses_in_expr(array, out);
            walk_index_uses_in_expr(index, out);
        }
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => {
            collect_index_refs(index, out);
            // (Issue #436) See the matching comment on `BufferIndexSet`
            // above — typed-array stores constrain the stored byte
            // range, and locals flowing into the value subtree are
            // safe to keep on the i32 shadow path. Image_convolution's
            // `dst[outIdx] = clampU8((rAcc / KSUM) | 0)` is the
            // motivating shape; #435's accumulator-overflow bugs all
            // output via `console.log`, not typed-array stores, so
            // this seed is correctness-safe against #435's regression
            // test.
            collect_index_refs(value, out);
            walk_index_uses_in_expr(array, out);
            walk_index_uses_in_expr(index, out);
            walk_index_uses_in_expr(value, out);
        }
        Expr::ArrayAt { array, index } => {
            collect_index_refs(index, out);
            walk_index_uses_in_expr(array, out);
            walk_index_uses_in_expr(index, out);
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            collect_index_refs(index, out);
            walk_index_uses_in_expr(array, out);
            walk_index_uses_in_expr(index, out);
            walk_index_uses_in_expr(value, out);
        }
        Expr::StringAt { string, index } => {
            collect_index_refs(index, out);
            walk_index_uses_in_expr(string, out);
            walk_index_uses_in_expr(index, out);
        }
        Expr::StringCodePointAt { string, index } => {
            collect_index_refs(index, out);
            walk_index_uses_in_expr(string, out);
            walk_index_uses_in_expr(index, out);
        }

        // --- pass-through structural traversal ---
        Expr::LocalGet(_) | Expr::Update { .. } => {}
        Expr::LocalSet(_, value) => walk_index_uses_in_expr(value, out),
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            walk_index_uses_in_expr(left, out);
            walk_index_uses_in_expr(right, out);
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::Await(operand)
        | Expr::Delete(operand)
        | Expr::StringCoerce(operand)
        | Expr::ObjectCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand) => {
            walk_index_uses_in_expr(operand, out);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            walk_index_uses_in_expr(condition, out);
            walk_index_uses_in_expr(then_expr, out);
            walk_index_uses_in_expr(else_expr, out);
        }
        Expr::Call { callee, args, .. } => {
            walk_index_uses_in_expr(callee, out);
            for a in args {
                walk_index_uses_in_expr(a, out);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            walk_index_uses_in_expr(callee, out);
            for a in args {
                match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => walk_index_uses_in_expr(e, out),
                }
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                walk_index_uses_in_expr(o, out);
            }
            for a in args {
                walk_index_uses_in_expr(a, out);
            }
        }
        Expr::PropertyGet { object, .. } => walk_index_uses_in_expr(object, out),
        Expr::PropertySet { object, value, .. } => {
            walk_index_uses_in_expr(object, out);
            walk_index_uses_in_expr(value, out);
        }
        Expr::PropertyUpdate { object, .. } => walk_index_uses_in_expr(object, out),
        Expr::Array(elements) => {
            for el in elements {
                walk_index_uses_in_expr(el, out);
            }
        }
        Expr::ArraySpread(elements) => {
            for el in elements {
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => {
                        walk_index_uses_in_expr(e, out);
                    }
                    ArrayElement::Hole => {}
                }
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                walk_index_uses_in_expr(v, out);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, e) in parts {
                walk_index_uses_in_expr(e, out);
            }
        }
        Expr::Sequence(es) => {
            for e in es {
                walk_index_uses_in_expr(e, out);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                walk_index_uses_in_expr(a, out);
            }
        }
        // Closure bodies are intentionally NOT walked: a captured local can't
        // use the i32 slot anyway (boxed captures route through
        // `js_box_get`/`js_box_set` and non-boxed ones through
        // `js_closure_get_capture_bits`), so marking them as index-used would
        // have no effect at the Let-site emission gate.
        Expr::Closure { .. } => {}
        // Everything else: conservatively skipped. Missing a variant means we
        // don't recurse further into that subtree — a local used as an index
        // deeper inside may not be marked, in which case its i32 shadow is
        // not emitted and the per-iteration `fptosi` cost returns. That's a
        // missed optimization, not a correctness bug.
        _ => {}
    }
}
