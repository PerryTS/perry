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
    payload: u32,
    next: u32,
    value: u32,
    mode: u32,
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
fn bound(stmts: &[Stmt], m: u32) -> Option<(u32, u32, u32, u32)> {
    for s in stmts {
        match s {
            Stmt::For {
                condition: Some(Expr::Compare { right, .. }),
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
                return Some((*payload, *next, *value, state));
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
        let Some((payload, next, value, _state)) = bound(&stmts[i + 1..], *id) else {
            continue;
        };
        // Only these compiler-owned HIR operands are redirected. An arbitrary
        // user catch/finally cannot manufacture this record boundary.
        let (base, fields) = ctx.func.alloca_entry_root_record(6);
        let r = Record {
            base,
            fields,
            payload,
            next,
            value,
            mode: *id,
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
    let field = if id == r.next {
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
    // Capture publishes next before its first read.
    let _ = init;

    Ok(true)
}
fn managed_field(r: &Record, id: u32) -> Option<usize> {
    if id == r.next {
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
    // Completion/exhaustion releases the range in the shared dispatcher.
    // The generated alias release must not write next again on every proven
    // array entry, where no protocol word was ever acquired.
    if !matches!(expr, Expr::Undefined) {
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
        "arrayRecordPayload" => {
            let Some(Expr::LocalGet(source)) = args.first() else {
                return Ok(None);
            };
            let Some(r) = ctx.array_stack_records.get(source).cloned() else {
                return Ok(None);
            };
            let value = lower_expr(ctx, &args[0])?;
            let flag = lower_expr(ctx, &Expr::LocalGet(r.mode))?;
            let bits = ctx.block().bitcast_double_to_i64(&flag);
            let needs = ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_TRUE_I64);
            let cold = ctx.new_block("record.capture");
            let done = ctx.new_block("record.captured");
            let cl = ctx.block_label(cold);
            let dl = ctx.block_label(done);
            let fast_pred = ctx.block().label.clone();
            ctx.block().cond_br(&needs, &cl, &dl);
            ctx.current_block = cold;
            ctx.block().store_volatile(DOUBLE, &value, &r.fields[0]);
            ctx.block().store(DOUBLE, "1.0", &r.fields[4]);
            dispatch(ctx, &r, "0");
            let cold_pred = ctx.block().label.clone();
            ctx.block().br(&dl);
            ctx.current_block = done;
            // The protocol owns its iterator in the stack record; the indexed
            // source local owns only the proven array. No duplicated source.
            let absent =
                crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let payload = ctx
                .block()
                .phi(DOUBLE, &[(&value, &fast_pred), (&absent, &cold_pred)]);
            if let Some(admission) = ctx.record_packed_admissions.remove(source) {
                ctx.record_packed_admissions.insert(r.payload, admission);
            }
            let _ = lower_expr(ctx, &args[2])?;
            return Ok(Some(payload));
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
            let flag = lower_expr(ctx, &Expr::LocalGet(r.mode))?;
            let bits = ctx.block().bitcast_double_to_i64(&flag);
            let protocol = ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_TRUE_I64);
            let array = lower_expr(ctx, &Expr::LocalGet(r.payload))?;
            let iterator = ctx.block().load_volatile(DOUBLE, &r.fields[0]);
            let payload =
                ctx.block()
                    .select(crate::types::I1, &protocol, DOUBLE, &iterator, &array);
            ctx.block().store_volatile(DOUBLE, &payload, &r.fields[0]);
            let flag = ctx.block().uitofp(crate::types::I1, &protocol, DOUBLE);
            ctx.block().store(DOUBLE, &flag, &r.fields[4]);
            let _ = lower_expr(ctx, &Expr::LocalSet(r.payload, Box::new(Expr::Undefined)))?;
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

/// Entry has proved an ordinary array on the indexed arm. Keep the private
/// binding's actual Any hint, and select the existing guarded-array backend
/// only for its compiler-owned unsigned record cursor.
pub(crate) fn indexed_source(ctx: &FnCtx<'_>, object: &Expr, index: &Expr) -> bool {
    let Expr::LocalGet(id) = object else {
        return false;
    };
    runtime(index, "arrayRecordIndex").is_some()
        && ctx
            .array_stack_records
            .get(id)
            .is_some_and(|r| r.payload == *id)
}
