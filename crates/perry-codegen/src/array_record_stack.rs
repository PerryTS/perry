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
    /// The source holds only sync generator objects on their birth layout
    /// whose protocol is the intrinsic one (`ElisionFacts::generator_sources`):
    /// each step resumes the body directly ([`generator_step`]). The record
    /// then has a seventh rooted word, the generator's home while its body
    /// runs (the payload word is clear then, as the dispatcher clears it).
    generator: bool,
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
                            && matches!(&v[2], Expr::IndexGet { object, index , .. }
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
        let generator = direct_generator(ctx, *source);
        let (base, fields) = ctx
            .func
            .alloca_entry_root_record(if generator { 7 } else { 6 });
        let r = Record {
            base,
            fields,
            source: *source,
            payload,
            next,
            value,
            mode: *id,
            fused,
            generator,
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
    // A call-free numeric region already owns a poll-refreshed descriptor.
    // Consume its promotable box just as an ordinary receiver binding does.
    // The record remains rooted, and the poll reloads the descriptor from this
    // payload home after evacuation; cold edges read the record directly.
    if field == 0 {
        if let Some(slot) = ctx
            .receiver_descriptors
            .rooted_box_slot(id)
            .map(str::to_owned)
        {
            return Some(ctx.block().load(DOUBLE, &slot));
        }
    }
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
/// Byte offset of control `i` of a fixed-state record (a `ClosureHeader`
/// followed by its captures; controls 0..=4 are state, done, sent,
/// executing and step kind: runtime `generator_state.rs`).
fn control(ctx: &mut FnCtx<'_>, rec: &str, i: u64) -> String {
    let off = crate::target_layout::closure_header_size_bytes(ctx.target_triple) + 8 * i;
    let blk = ctx.block();
    let addr = blk.add(I64, rec, &off.to_string());
    blk.inttoptr(I64, &addr)
}

/// The record (`[[GeneratorState]]` frame) of the generator in the rooted
/// word `home`: the first slot of its birth layout, which the facts prove
/// it keeps (no own key). Read afresh after every call that may collect.
fn generator_record(ctx: &mut FnCtx<'_>, home: &str) -> String {
    let base = crate::target_layout::object_slot_base_bytes(
        ctx.target_triple,
        crate::target_layout::HeaderForm::Meta,
    );
    let blk = ctx.block();
    let g = blk.load_volatile(DOUBLE, home);
    let g = blk.bitcast_double_to_i64(&g);
    let g = blk.and(I64, &g, crate::nanbox::POINTER_MASK_I64);
    let slot = blk.add(I64, &g, &base.to_string());
    let slot = blk.inttoptr(I64, &slot);
    let rec = blk.load(I64, &slot);
    blk.and(I64, &rec, crate::nanbox::POINTER_MASK_I64)
}

/// One for-of step of a proven record ([`generator_resume`]) with the record
/// words as the dispatcher leaves them: payload clear while the step runs
/// (a throwing step closes nothing, 14.7.5.7), back on a value step, clear
/// when done. Returns (done as a NaN-boxed boolean, value).
fn generator_step(ctx: &mut FnCtx<'_>, r: &Record) -> (String, String) {
    let undefined = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let home = r.fields[6].clone();
    let payload = ctx.block().load_volatile(DOUBLE, &r.fields[0]);
    ctx.block().store_volatile(DOUBLE, &payload, &home);
    ctx.block().store_volatile(DOUBLE, &undefined, &r.fields[0]);
    let (done, value) = generator_resume(ctx, r.source, &home, &r.fields[5]);
    let g = ctx.block().load_volatile(DOUBLE, &home);
    let bits = ctx.block().bitcast_double_to_i64(&done);
    let finished = ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_TRUE_I64);
    let payload = ctx
        .block()
        .select(crate::types::I1, &finished, DOUBLE, &undefined, &g);
    ctx.block().store_volatile(DOUBLE, &payload, &r.fields[0]);
    ctx.block().store_volatile(DOUBLE, &undefined, &home);
    (done, value)
}

/// Whether `source` holds only proven sync generators whose for-of step
/// resumes the body directly ([`generator_resume`]).
pub(crate) fn direct_generator(ctx: &FnCtx<'_>, source: u32) -> bool {
    !ctx.wpfacts_off
        && !ctx.target_triple.contains("wasm")
        && ctx.wpfacts.generator_sources.contains(&source)
}

/// IteratorStepValue of `%GeneratorPrototype%.next` on the generator in the
/// rooted word `home` (ECMA-262 7.4.8, 27.5.3.3 GeneratorValidate /
/// GeneratorResume), as runtime `resume_fixed_step` takes `next(undefined)`:
/// executing → TypeError; done → done; else executing, sent = undefined, the
/// body, not executing. The body's own completion catch marks the
/// generator done on a throw, so no trap frame is needed here. `source` is
/// the local the generator came from (its proven bodies); `out` a frame
/// slot the cold `yield*` inner-result step writes its value to. `home`
/// still holds the generator on return. Returns (done as a NaN-boxed
/// boolean, value).
pub(crate) fn generator_resume(
    ctx: &mut FnCtx<'_>,
    source: u32,
    home: &str,
    out: &str,
) -> (String, String) {
    let undefined = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let true_d = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_TRUE));
    let false_d = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_FALSE));
    let rec = generator_record(ctx, home);
    let executing = control(ctx, &rec, 3);
    let running = ctx.block().load(I64, &executing);
    let running = ctx
        .block()
        .icmp_eq(I64, &running, crate::nanbox::TAG_TRUE_I64);
    let busy_idx = ctx.new_block("gen.executing");
    let check_idx = ctx.new_block("gen.check_done");
    let (busy_label, check_label) = (ctx.block_label(busy_idx), ctx.block_label(check_idx));
    ctx.block().cond_br(&running, &busy_label, &check_label);
    ctx.current_block = busy_idx;
    let _ = ctx
        .block()
        .call(DOUBLE, "js_generator_resume_executing", &[]);
    ctx.block().unreachable();

    ctx.current_block = check_idx;
    let done_ptr = control(ctx, &rec, 1);
    let finished = ctx.block().load(I64, &done_ptr);
    let finished = ctx
        .block()
        .icmp_eq(I64, &finished, crate::nanbox::TAG_TRUE_I64);
    let resume_idx = ctx.new_block("gen.resume");
    let join_idx = ctx.new_block("gen.stepped");
    let (resume_label, join_label) = (ctx.block_label(resume_idx), ctx.block_label(join_idx));
    let finished_pred = ctx.block().label.clone();
    ctx.block().cond_br(&finished, &join_label, &resume_label);

    ctx.current_block = resume_idx;
    ctx.block()
        .store(I64, crate::nanbox::TAG_TRUE_I64, &executing);
    let sent = control(ctx, &rec, 2);
    ctx.block()
        .store(I64, crate::nanbox::TAG_UNDEFINED_I64, &sent);
    let bodies = ctx.wpfacts.local_bodies.get(&source).cloned();
    let call_body = |ctx: &mut FnCtx<'_>, body: crate::expr::body_call::JsBody<'_>, params: u32| {
        let args: Vec<String> = if params == 1 {
            vec![undefined.clone()]
        } else {
            vec!["0.0".to_string(), undefined.clone()]
        };
        crate::expr::body_call::emit_js_body_call(
            ctx.block(),
            body,
            &rec,
            crate::expr::body_call::JS_THIS_UNDEFINED,
            &args,
        )
    };
    let result = match bodies.as_deref() {
        Some([b]) if b.closure => {
            let sym = crate::fn_info::closure_body_symbol(&ctx.strings.module_prefix(), b.func_id);
            call_body(ctx, crate::expr::body_call::JsBody::Symbol(&sym), b.params)
        }
        _ => {
            // The code word of the record's info; its parameter count picks
            // the body ABI, as the runtime boundary reads it.
            let info_off = crate::target_layout::closure_info_offset_bytes(ctx.target_triple);
            // `JsFunctionInfo::params` follows the `code` pointer.
            let ptr_bytes = if crate::target_layout::target_is_ilp32(ctx.target_triple) {
                4
            } else {
                8
            };
            let blk = ctx.block();
            let info_addr = blk.add(I64, &rec, &info_off.to_string());
            let info_ptr = blk.inttoptr(I64, &info_addr);
            let info = blk.load(I64, &info_ptr);
            let code_ptr = blk.inttoptr(I64, &info);
            let code = blk.load(PTR, &code_ptr);
            let params_addr = blk.add(I64, &info, &ptr_bytes.to_string());
            let params_ptr = blk.inttoptr(I64, &params_addr);
            let params = blk.load(crate::types::I16, &params_ptr);
            let one = blk.icmp_eq(crate::types::I16, &params, "1");
            let one_idx = ctx.new_block("gen.body1");
            let two_idx = ctx.new_block("gen.body2");
            let called_idx = ctx.new_block("gen.called");
            let (one_label, two_label, called_label) = (
                ctx.block_label(one_idx),
                ctx.block_label(two_idx),
                ctx.block_label(called_idx),
            );
            ctx.block().cond_br(&one, &one_label, &two_label);
            ctx.current_block = one_idx;
            let v1 = call_body(ctx, crate::expr::body_call::JsBody::Pointer(&code), 1);
            let p1 = ctx.block().label.clone();
            ctx.block().br(&called_label);
            ctx.current_block = two_idx;
            let v2 = call_body(ctx, crate::expr::body_call::JsBody::Pointer(&code), 2);
            let p2 = ctx.block().label.clone();
            ctx.block().br(&called_label);
            ctx.current_block = called_idx;
            ctx.block().phi(DOUBLE, &[(&v1, &p1), (&v2, &p2)])
        }
    };
    // The body may have collected: the record is read again from the home.
    let rec = generator_record(ctx, home);
    let executing = control(ctx, &rec, 3);
    ctx.block()
        .store(I64, crate::nanbox::TAG_FALSE_I64, &executing);
    // A string the record may append to in place is aliased by the loop
    // variable: demote a uniquely owned one, as the runtime boundary does.
    let bits = ctx.block().bitcast_double_to_i64(&result);
    let tag = ctx.block().and(
        I64,
        &bits,
        &crate::nanbox::i64_literal(crate::nanbox::TAG_MASK),
    );
    let is_str = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::STRING_TAG_I64);
    let str_idx = ctx.new_block("gen.str");
    let kind_idx = ctx.new_block("gen.kind");
    let (str_label, kind_label) = (ctx.block_label(str_idx), ctx.block_label(kind_idx));
    ctx.block().cond_br(&is_str, &str_label, &kind_label);
    ctx.current_block = str_idx;
    let ptr = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    ctx.block().call_void("js_string_addref", &[(I64, &ptr)]);
    ctx.block().br(&kind_label);

    ctx.current_block = kind_idx;
    let kind_ptr = control(ctx, &rec, 4);
    let kind = ctx.block().load(DOUBLE, &kind_ptr);
    let yielded = ctx.block().fcmp("oeq", &kind, "0.0");
    let other_idx = ctx.new_block("gen.not_yield");
    let other_label = ctx.block_label(other_idx);
    let yield_pred = ctx.block().label.clone();
    ctx.block().cond_br(&yielded, &join_label, &other_label);

    ctx.current_block = other_idx;
    let completed = ctx.block().fcmp("oeq", &kind, "1.0");
    let inner_idx = ctx.new_block("gen.inner_result");
    let completed_pred = ctx.block().label.clone();
    let inner_label = ctx.block_label(inner_idx);
    ctx.block().cond_br(&completed, &join_label, &inner_label);

    // A `yield*` inner result object, re-yielded as is: its done and value
    // by ordinary [[Get]] (cold), the generator rooted in its home.
    ctx.current_block = inner_idx;
    let inner_done = ctx.block().call(
        I32,
        "js_iterator_step_inner_result",
        &[(DOUBLE, &result), (PTR, out)],
    );
    let inner_value = ctx.block().load_volatile(DOUBLE, out);
    let inner_is_done = ctx.block().icmp_ne(I32, &inner_done, "0");
    let inner_done_d =
        ctx.block()
            .select(crate::types::I1, &inner_is_done, DOUBLE, &true_d, &false_d);
    let inner_v = ctx.block().select(
        crate::types::I1,
        &inner_is_done,
        DOUBLE,
        &undefined,
        &inner_value,
    );
    let inner_pred = ctx.block().label.clone();
    ctx.block().br(&join_label);

    ctx.current_block = join_idx;
    let done = ctx.block().phi(
        DOUBLE,
        &[
            (&true_d, &finished_pred),
            (&false_d, &yield_pred),
            (&true_d, &completed_pred),
            (&inner_done_d, &inner_pred),
        ],
    );
    let value = ctx.block().phi(
        DOUBLE,
        &[
            (&undefined, &finished_pred),
            (&result, &yield_pred),
            (&undefined, &completed_pred),
            (&inner_v, &inner_pred),
        ],
    );
    (done, value)
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
            let writable = crate::expr::array_iteration_facts(&ctx.wpfacts).to_string();
            let verdict = ctx.block().call(
                I32,
                "js_array_record_stack_enter",
                &[
                    (PTR, &r.base),
                    (I32, &writable),
                    (I32, if counted { "1" } else { "0" }),
                ],
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
                    let (done, value) = if r.generator {
                        crate::facts_consumed::used();
                        generator_step(ctx, &r)
                    } else {
                        let done = dispatch(ctx, &r, "1");
                        (done, ctx.block().load_volatile(DOUBLE, &r.fields[5]))
                    };
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
            // #13645: with no `return` on the array iterator chain only the
            // protocol arm completes through the dispatcher; the array arm's
            // IteratorClose is unobservable, and the plan's exit release
            // ends the record's custody of the payload.
            let elide = method == "arrayRecordFinish" && ctx.wpfacts.array_iterator_close_unwritten;
            let throwing = matches!(args[6], Expr::Bool(true));
            let mut skip = None;
            if elide {
                crate::verify_facts::consumed(crate::verify_facts::Fact::IteratorClose, || {
                    serde_json::json!({})
                });
                let skipped = if throwing {
                    lower_expr(ctx, &args[5])?
                } else {
                    crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let flag = lower_expr(ctx, &Expr::LocalGet(r.mode))?;
                let bits = ctx.block().bitcast_double_to_i64(&flag);
                let protocol = ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_TRUE_I64);
                let close_idx = ctx.new_block("arec.close");
                let join_idx = ctx.new_block("arec.closed");
                let (close_label, join_label) =
                    (ctx.block_label(close_idx), ctx.block_label(join_idx));
                let skip_pred = ctx.block().label.clone();
                ctx.block().cond_br(&protocol, &close_label, &join_label);
                ctx.current_block = close_idx;
                skip = Some((join_idx, skipped, skip_pred));
            }
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
            if let Some((join_idx, skipped, skip_pred)) = skip {
                let close_pred = ctx.block().label.clone();
                let join_label = ctx.block_label(join_idx);
                ctx.block().br(&join_label);
                ctx.current_block = join_idx;
                let value = ctx
                    .block()
                    .phi(DOUBLE, &[(&value, &close_pred), (&skipped, &skip_pred)]);
                return Ok(Some(value));
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
    if r.generator {
        // The source holds only generator objects (an `undefined` or `null`
        // throws when the loop starts), so the record is in protocol mode
        // and no array arm can run: the step is the generator's.
        let index = lower_expr(ctx, &args[2])?;
        ctx.block().store(DOUBLE, &index, &r.fields[2]);
        crate::facts_consumed::used();
        let (done, value) = generator_step(ctx, &r);
        let bits = ctx.block().bitcast_double_to_i64(&done);
        let ready = ctx
            .block()
            .icmp_eq(I64, &bits, crate::nanbox::TAG_FALSE_I64);
        crate::expr::invalidate_local_write_facts(ctx, r.value);
        crate::expr::bind_lowered_value_to_local(ctx, r.value, &value, &Expr::LocalGet(r.value))?;
        return Ok(Some(ready));
    }
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
