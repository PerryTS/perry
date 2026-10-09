//! Stack ABI for compiler-owned for-of records. The existing HIR remains the
//! semantic plan; its private fields share one GC home and one cold dispatcher.
use crate::expr::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I32, I64, PTR};
use anyhow::Result;
use perry_hir::{CompareOp, Expr, Stmt};

#[derive(Clone)]
pub(crate) struct Record {
    base: String,
    fields: Vec<String>,
    source: u32,
    payload: u32,
    next: u32,
    value: u32,
    mode: u32,
    fused: bool,
}
fn runtime<'a>(e: &'a Expr, method: &str) -> Option<&'a [Expr]> {
    match e {
        Expr::NativeMethodCall {
            module, method: m, ..
        } if module == "__perry_runtime" && m == method => {
            let Expr::NativeMethodCall { args, .. } = e else {
                unreachable!()
            };
            Some(args)
        }
        _ => None,
    }
}
fn mode(e: &Expr) -> Option<u32> {
    match e {
        Expr::Compare {
            op: CompareOp::Eq,
            left,
            right,
        } if matches!(right.as_ref(), Expr::Bool(true)) => {
            if let Expr::LocalGet(id) = left.as_ref() {
                Some(*id)
            } else {
                None
            }
        }
        Expr::LocalGet(id) => Some(*id),
        _ => None,
    }
}
fn bound(stmts: &[Stmt], m: u32) -> Option<(u32, u32, u32, u32, bool)> {
    for s in stmts {
        match s {
            Stmt::For {
                init,
                condition: Some(Expr::Compare { right, .. }),
                body,
                update,
                ..
            } => {
                let Some(a) = runtime(right, "arrayRecordForBound") else {
                    continue;
                };
                if a.len() != 5 || mode(&a[0]) != Some(m) {
                    continue;
                }
                let step = runtime(&a[3], "iteratorStep")?;
                let [Expr::LocalGet(payload), Expr::LocalGet(next), Expr::LocalSet(value, _), ..] =
                    step
                else {
                    return None;
                };
                let Expr::LocalSet(state, _) = a[4] else {
                    return None;
                };
                let fused = matches!(a.get(2), Some(Expr::LocalGet(counter))
                    if matches!(init.as_deref(), Some(Stmt::Let { id, init: Some(e), .. })
                        if id == counter && runtime(e, "arrayRecordIndex").is_some_and(|v| matches!(v, [Expr::Integer(0)])))
                    && matches!(body.first(), Some(Stmt::Expr(Expr::LocalSet(id, e)))
                        if id == value && runtime(e, "arrayRecordForValue").is_some_and(|v| v.len() == 3 && mode(&v[0]) == Some(m)
                            && matches!(&v[1], Expr::LocalGet(id) if id == value)
                            && matches!(&v[2], Expr::IndexGet { object, index }
                                if matches!(object.as_ref(), Expr::LocalGet(id) if id == payload)
                                && runtime(index, "arrayRecordIndex").is_some_and(|v| matches!((v, &a[2]), ([Expr::LocalGet(id)], Expr::LocalGet(counter)) if id == counter)))))
                    && runtime(&a[1], "arrayRecordLength").is_some_and(|v| matches!(v, [Expr::LocalGet(id)] if id == payload))
                    && update.as_ref().and_then(|e| runtime(e, "arrayRecordForUpdate")).is_some_and(|v| v.len() == 2 && mode(&v[0]) == Some(m)
                        && matches!(&v[1], Expr::Update { id, op: perry_hir::UpdateOp::Increment, prefix: false }
                            if matches!(&a[2], Expr::LocalGet(counter) if counter == id))));
                return Some((*payload, *next, *value, state, fused));
            }
            Stmt::Try { body, .. } => {
                if let Some(b) = bound(body, m) {
                    return Some(b);
                }
            }
            _ => {}
        }
    }
    None
}
pub(crate) fn prepare(ctx: &mut FnCtx<'_>, stmts: &[Stmt]) {
    for (i, s) in stmts.iter().enumerate() {
        let Stmt::Let {
            id, init: Some(e), ..
        } = s
        else {
            continue;
        };
        let Some(args) = runtime(e, "arrayRecordNeedsIterator") else {
            continue;
        };
        let Some(Expr::LocalSet(source, _)) = args.first() else {
            continue;
        };
        if ctx.array_stack_records.contains_key(source) {
            continue;
        }
        let Some((payload, next, value, _state, fused)) = bound(&stmts[i + 1..], *id) else {
            continue;
        };
        // Only these compiler-owned HIR operands are redirected. An arbitrary
        // user catch/finally cannot manufacture this record boundary.
        let (base, fields) = ctx.func.alloca_entry_root_record(6);
        let r = Record {
            base,
            fields,
            source: *source,
            payload,
            next,
            value,
            mode: *id,
            fused,
        };
        for local in [*source, *id, payload, next, value] {
            ctx.array_stack_records.insert(local, r.clone());
        }
    }
}
pub(crate) fn bind_field(
    ctx: &mut FnCtx<'_>,
    id: u32,
    name: &str,
    init: Option<&Expr>,
    ty: &perry_hir::types::Type,
) -> Result<bool> {
    let Some(r) = ctx.array_stack_records.get(&id).cloned() else {
        return Ok(false);
    };
    let field = if id == r.payload || id == r.source {
        0
    } else if id == r.next {
        1
    } else {
        return Ok(false);
    };
    ctx.local_id_to_name.insert(id, name.into());
    ctx.local_types.insert(id, ty.clone());
    // The range owns these words directly. Binding a second named-local root
    // would duplicate custody and make RS4GC mistake a field GEP for an alloca.
    ctx.shadow_slot_map.remove(&id);
    ctx.locals.insert(id, r.fields[field].clone());
    // Payload acquisition owns the record before either source is consumed.
    // Captured next was published by that acquisition.
    if id == r.payload {
        if let Some(init) = init {
            let _ = lower_expr(ctx, init)?;
        }
    } else if id == r.source {
        if let Some(init) = init {
            let value = lower_expr(ctx, init)?;
            ctx.block().store_volatile(DOUBLE, &value, &r.fields[0]);
        }
    }

    Ok(true)
}
fn managed_field(r: &Record, id: u32) -> Option<usize> {
    if id == r.payload || id == r.source {
        Some(0)
    } else if id == r.next {
        Some(1)
    } else {
        None
    }
}
pub(crate) fn read_local(ctx: &mut FnCtx<'_>, id: u32) -> Option<String> {
    let r = ctx.array_stack_records.get(&id)?.clone();
    let field = managed_field(&r, id)?;
    Some(ctx.block().load_volatile(DOUBLE, &r.fields[field]))
}
pub(crate) fn write_local(ctx: &mut FnCtx<'_>, id: u32, value: &str, expr: &Expr) -> bool {
    let Some(r) = ctx.array_stack_records.get(&id).cloned() else {
        return false;
    };
    let Some(field) = managed_field(&r, id) else {
        return false;
    };
    // The payload home is rooted at every safepoint of the function, not only
    // while the loop runs, so the plan's exit release is what ends the
    // traversal's custody of the source on every completion: exhaustion,
    // break/return and throw. The protocol dispatcher has already cleared it
    // on its own completions; a proven array completes without the
    // dispatcher. The source alias releases inside payload acquisition,
    // where the same field already holds the payload, and the next word is
    // only ever written and released by the dispatcher.
    if !matches!(expr, Expr::Undefined) || (id == r.payload && id != r.source) {
        ctx.block().store_volatile(DOUBLE, value, &r.fields[field]);
    }
    true
}
fn dispatch(ctx: &mut FnCtx<'_>, r: &Record, op: &str) -> String {
    ctx.block().call(
        DOUBLE,
        "js_array_record_stack_dispatch",
        &[(PTR, &r.base), (I32, op)],
    )
}
pub(crate) fn lower(ctx: &mut FnCtx<'_>, method: &str, args: &[Expr]) -> Result<Option<String>> {
    match method {
        "arrayRecordNeedsIterator" => {
            let Some(Expr::LocalSet(source, input)) = args.first() else {
                return Ok(None);
            };
            let Some(r) = ctx.array_stack_records.get(source).cloned() else {
                return Ok(None);
            };
            let value = lower_expr(ctx, input)?;
            ctx.block().store_volatile(DOUBLE, &value, &r.fields[0]);
            let counted = matches!(args.get(1), Some(Expr::Bool(true)));
            let site = crate::expr::array_record_site(ctx);
            let verdict = ctx.block().call(
                I32,
                if counted {
                    "js_array_record_enter_counted"
                } else {
                    "js_array_record_enter"
                },
                &[(DOUBLE, &value), (PTR, &r.fields[0]), (PTR, &site)],
            );
            let needs = if counted {
                let admission = ctx
                    .record_packed_admissions
                    .get(source)
                    .cloned()
                    .unwrap_or_else(|| ctx.func.alloca_entry(crate::types::I1));
                let bit = ctx.block().and(I32, &verdict, "2");
                let admitted = ctx.block().icmp_ne(I32, &bit, "0");
                ctx.block().store(crate::types::I1, &admitted, &admission);
                ctx.record_packed_admissions.insert(*source, admission);
                ctx.block().and(I32, &verdict, "1")
            } else {
                verdict
            };
            return Ok(Some(crate::expr::i32_bool_to_nanbox(ctx.block(), &needs)));
        }
        "arrayRecordForUpdate" => {
            if args
                .first()
                .and_then(mode)
                .and_then(|id| ctx.array_stack_records.get(&id))
                .is_some_and(|r| r.fused)
            {
                // Protocol completion does not observe the private counted
                // cursor. Advancing it uniformly removes a per-entry mode
                // branch without changing the captured IteratorRecord.
                return Ok(Some(lower_expr(ctx, &args[1])?));
            }
        }
        "arrayRecordPayload" => {
            let Some(Expr::LocalGet(source)) = args.first() else {
                return Ok(None);
            };
            let Some(r) = ctx.array_stack_records.get(source).cloned() else {
                return Ok(None);
            };
            let flag = lower_expr(ctx, &Expr::LocalGet(r.mode))?;
            let bits = ctx.block().bitcast_double_to_i64(&flag);
            let needs = ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_TRUE_I64);
            let flag = ctx.block().uitofp(crate::types::I1, &needs, DOUBLE);
            ctx.block().store(DOUBLE, &flag, &r.fields[4]);
            let cold = ctx.new_block("record.capture");
            let done = ctx.new_block("record.captured");
            let cl = ctx.block_label(cold);
            let dl = ctx.block_label(done);
            ctx.block().cond_br(&needs, &cl, &dl);
            ctx.current_block = cold;
            dispatch(ctx, &r, "0");
            ctx.block().br(&dl);
            ctx.current_block = done;
            let payload = ctx.block().load_volatile(DOUBLE, &r.fields[0]);
            if let Some(admission) = ctx.record_packed_admissions.remove(source) {
                ctx.record_packed_admissions.insert(r.payload, admission);
            }
            let _ = lower_expr(ctx, &args[2])?;
            return Ok(Some(payload));
        }
        "arrayRecordForValue" => {
            if let Some(r) = args
                .first()
                .and_then(mode)
                .and_then(|id| ctx.array_stack_records.get(&id))
                .filter(|r| r.fused)
                .cloned()
            {
                return Ok(Some(lower_expr(ctx, &Expr::LocalGet(r.value))?));
            }
        }
        "iteratorNextMethod" => {
            if let Some(Expr::LocalGet(id)) = args.first() {
                if let Some(r) = ctx.array_stack_records.get(id).cloned() {
                    return Ok(Some(ctx.block().load_volatile(DOUBLE, &r.fields[1])));
                }
            }
        }
        "iteratorStep" => {
            if let Some(Expr::LocalGet(id)) = args.first() {
                if let Some(r) = ctx.array_stack_records.get(id).cloned() {
                    let done = dispatch(ctx, &r, "1");
                    let value = ctx.block().load_volatile(DOUBLE, &r.fields[5]);
                    let Expr::LocalSet(value_id, output_expr) = &args[2] else {
                        anyhow::bail!("stack record Step needs its output binding");
                    };
                    debug_assert_eq!(*value_id, r.value);
                    // Transfer the protocol result to the normal binding. The
                    // one user body keeps its usual SSA rooting on both arms;
                    // the fast indexed value never touches a record field.
                    crate::expr::invalidate_local_write_facts(ctx, *value_id);
                    crate::expr::bind_lowered_value_to_local(ctx, *value_id, &value, output_expr)?;
                    let absent =
                        crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                    ctx.block().store_volatile(DOUBLE, &absent, &r.fields[5]);
                    return Ok(Some(done));
                }
            }
        }
        "arrayRecordFinish" | "arrayRecordAbrupt" => {
            let Some(id) = args.first().and_then(mode) else {
                return Ok(None);
            };
            let Some(r) = ctx.array_stack_records.get(&id).cloned() else {
                return Ok(None);
            };
            // Payload and traversal mode already live in the range. The
            // cold edge writes only its cursor and completion state.
            let index = lower_expr(ctx, &args[2])?;
            ctx.block().store(DOUBLE, &index, &r.fields[2]);
            // The recognized abrupt handler may wrap state in an exit guard.
            if method == "arrayRecordAbrupt" {
                let state = lower_expr(ctx, &args[4])?;
                ctx.block().store(DOUBLE, &state, &r.fields[3]);
            } else {
                let done = lower_expr(ctx, &args[4])?;
                let bits = ctx.block().bitcast_double_to_i64(&done);
                let done = ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_TRUE_I64);
                let closed = ctx
                    .block()
                    .select(crate::types::I1, &done, DOUBLE, "2.0", "0.0");
                ctx.block().store(DOUBLE, &closed, &r.fields[3]);
            }
            let op = if method == "arrayRecordAbrupt" {
                "3"
            } else if matches!(args[6], Expr::Bool(true)) {
                let error = lower_expr(ctx, &args[5])?;
                ctx.block().store_volatile(DOUBLE, &error, &r.fields[5]);
                "4"
            } else {
                "2"
            };
            let value = dispatch(ctx, &r, op);
            if method == "arrayRecordAbrupt" {
                ctx.block().unreachable();
            }
            return Ok(Some(value));
        }
        _ => {}
    }
    Ok(None)
}

/// The existing loop planner consumes its raw-load proof before this hook.
/// A remaining record read keeps the existing guarded array word/load, while
/// forwarding, descriptors, prototypes and holes use the one cold dispatcher.
pub(crate) fn read(ctx: &mut FnCtx<'_>, object: &Expr, index: &Expr) -> Result<Option<String>> {
    let Expr::LocalGet(id) = object else {
        return Ok(None);
    };
    let Some(r) = ctx
        .array_stack_records
        .get(id)
        .filter(|r| r.payload == *id)
        .cloned()
    else {
        return Ok(None);
    };
    let value = crate::rooting::with_operands_rooted(ctx, &[object, index], |ctx, values| {
        crate::expr::lower_record_index(
            ctx,
            &values[0],
            &values[1],
            &r.base,
            &r.fields[0],
            &r.fields[2],
        )
    })?;
    Ok(Some(value))
}

// The exact generated loop consumes its value immediately after this test.
// Both branches bind that one value; the original user body remains intact.
pub(crate) fn next_test(ctx: &mut FnCtx<'_>, args: &[Expr]) -> Result<Option<String>> {
    let Some(r) = args
        .first()
        .and_then(mode)
        .and_then(|id| ctx.array_stack_records.get(&id))
        .filter(|r| r.fused)
        .cloned()
    else {
        return Ok(None);
    };
    lower_expr(ctx, &args[4])?;
    let array = lower_expr(ctx, &Expr::LocalGet(r.payload))?;
    let index = lower_expr(ctx, &args[2])?;
    let protocol = crate::lower_conditional::lower_test(ctx, &args[0])?;
    let (value, ready) = crate::expr::lower_record_next(
        ctx,
        &array,
        &index,
        &protocol,
        &r.base,
        &r.fields[2],
        &r.fields[5],
    )?;
    crate::expr::invalidate_local_write_facts(ctx, r.value);
    crate::expr::bind_lowered_value_to_local(ctx, r.value, &value, &Expr::LocalGet(r.value))?;
    Ok(Some(ready))
}

// Step bound the output for both representations; do not repeat the old
// indexed transfer when the generated loop has that fused boundary.
pub(crate) fn has_fused_value(ctx: &FnCtx<'_>, id: u32) -> bool {
    ctx.array_stack_records
        .get(&id)
        .is_some_and(|r| r.fused && r.value == id)
}

pub(crate) fn fused_condition(ctx: &FnCtx<'_>, condition: Option<&Expr>) -> bool {
    let Some(Expr::Compare {
        op: CompareOp::Lt,
        right,
        ..
    }) = condition
    else {
        return false;
    };
    runtime(right, "arrayRecordForBound")
        .and_then(|args| args.first())
        .and_then(mode)
        .and_then(|id| ctx.array_stack_records.get(&id))
        .is_some_and(|r| r.fused)
}
