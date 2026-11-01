//! Expose the proven indexed step to the existing counted-loop matcher.
//! The source type is only a candidate hint: the captured shape verdict and
//! the existing packed receiver/element guard both gate the native copy.
use super::*;
use perry_hir::{CompareOp, Expr};

pub(super) fn lower(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Result<bool> {
    let Some(Expr::Compare {
        op: CompareOp::Lt,
        left,
        right,
    }) = condition
    else {
        return Ok(false);
    };
    let Expr::LocalGet(counter) = left.as_ref() else {
        return Ok(false);
    };
    let Expr::NativeMethodCall {
        module,
        class_name: None,
        object: None,
        method,
        args,
    } = right.as_ref()
    else {
        return Ok(false);
    };
    if module != "__perry_runtime" || method != "arrayRecordForBound" || args.len() != 5 {
        return Ok(false);
    }
    let Expr::NativeMethodCall {
        module,
        method,
        args: length_args,
        ..
    } = &args[1]
    else {
        return Ok(false);
    };
    if module != "__perry_runtime" || method != "arrayRecordLength" {
        return Ok(false);
    }
    let [Expr::LocalGet(array)] = length_args.as_slice() else {
        return Ok(false);
    };
    let array = *array;
    if !matches!(args[2], Expr::LocalGet(id) if id == *counter) || body.len() < 2 {
        return Ok(false);
    }
    let Stmt::Expr(Expr::LocalSet(output, value)) = &body[0] else {
        return Ok(false);
    };
    let Expr::NativeMethodCall {
        module,
        method,
        args: values,
        ..
    } = value.as_ref()
    else {
        return Ok(false);
    };
    if module != "__perry_runtime"
        || method != "arrayRecordForValue"
        || values.len() != 3
        || mode_local(&values[0]).is_none()
        || mode_local(&values[0]) != mode_local(&args[0])
    {
        return Ok(false);
    }
    let Some(Stmt::Let {
        id,
        init:
            Some(Expr::NativeMethodCall {
                module,
                method,
                args: init_args,
                ..
            }),
        ..
    }) = init
    else {
        return Ok(false);
    };
    if id != counter
        || module != "__perry_runtime"
        || method != "arrayRecordIndex"
        || !matches!(init_args.as_slice(), [Expr::Integer(0)])
    {
        return Ok(false);
    }
    let candidate_init = Stmt::Let {
        id: *counter,
        name: String::new(),
        ty: perry_hir::types::Type::Number,
        mutable: true,
        init: Some(Expr::Integer(0)),
    };
    let candidate_condition = Expr::Compare {
        op: CompareOp::Lt,
        left: Box::new(Expr::LocalGet(*counter)),
        right: Box::new(Expr::PropertyGet {
            byte_end: 0,
            object: Box::new(Expr::LocalGet(array)),
            property: "length".into(),
            byte_offset: 0,
        }),
    };
    let element = Expr::IndexGet {
        byte_offset: 0,
        byte_end: 0,
        object: Box::new(Expr::LocalGet(array)),
        index: Box::new(Expr::LocalGet(*counter)),
    };
    let mut candidate_body = body[2..].to_vec();
    // Successful packed admission proves this read pure and numeric. The
    // ordinary matcher must independently prove that every consumer statement
    // is call-free and cannot mutate the receiver/length. Only that native
    // copy can substitute the transfer; the protocol keeps its output slot.
    let substituted = candidate_body
        .iter_mut()
        .all(|stmt| substitute_output(stmt, *output, &element));
    let Some(Expr::NativeMethodCall {
        module,
        method,
        args: updates,
        ..
    }) = update
    else {
        return Ok(false);
    };
    if module != "__perry_runtime"
        || method != "arrayRecordForUpdate"
        || updates.len() != 2
        || mode_local(&updates[0]) != mode_local(&args[0])
    {
        return Ok(false);
    }
    let was_nonnegative = ctx.nonnegative_integer_locals.contains(counter);
    ctx.nonnegative_integer_locals.insert(*counter);
    let mut result = if substituted {
        lower_packed_f64_versioned_for_with_protocol(
            ctx,
            Some(&candidate_init),
            Some(&candidate_condition),
            Some(&updates[1]),
            &candidate_body,
            Some((&values[0], condition.unwrap(), update.unwrap(), body)),
        )
    } else {
        Ok(false)
    };
    if matches!(result, Ok(false)) && ctx.pending_labels.is_empty() {
        if let Expr::LocalSet(state, _) = &args[4] {
            result = lower_array_arm(
                ctx,
                &values[0],
                ArrayArm {
                    condition: &candidate_condition,
                    array,
                    counter: *counter,
                    output: *output,
                    state: *state,
                    length: &args[1],
                    element: &values[2],
                    body,
                },
                (condition.unwrap(), update.unwrap()),
            );
        }
    }
    if !was_nonnegative {
        ctx.nonnegative_integer_locals.remove(counter);
    }
    result
}

struct ArrayArm<'a> {
    condition: &'a Expr,
    array: u32,
    counter: u32,
    output: u32,
    state: u32,
    /// The record's own length: `arrayRecordLength(payload)`.
    length: &'a Expr,
    /// The record's own Get: `payload[arrayRecordIndex(counter)]`.
    element: &'a Expr,
    /// The record body: output transfer, `state = 0`, then the user's.
    body: &'a [Stmt],
}

/// A counted record whose entry chose the array arm steps exactly like the
/// indexed loop: the captured %ArrayIteratorPrototype%.next reads `a[i]`
/// below the live length (ECMA-262 23.1.5.1). The two arms differ only in
/// how a step produces the value, so the loop has one body and two heads:
///
///   head:     protocol ? protocol.step : indexed.step
///   indexed:  state = 2; i < payload.length (typed) ? output = payload[i]
///   protocol: the record's own Step (state = 2, IteratorStepValue)
///   body:     state = 0; the user body (one copy)
///   update:   the record's own (i++ on the indexed head)
///
/// The mode is tested once before the loop; each head branches on that one
/// value, so the per-step work of the indexed head is the indexed loop's.
/// ECMA-262 order is the record's on both heads: completion state 2 while
/// Step/Get runs (a throwing getter or `next` completes the record without
/// IteratorClose), 0 before the user body (a break or throw there closes),
/// and exhaustion leaves 2 for the plan's release. Both heads share the
/// cursor, completion state and output locals, so close, abrupt completion
/// and release see the same record either way.
fn lower_array_arm(
    ctx: &mut FnCtx<'_>,
    protocol_test: &Expr,
    arm: ArrayArm<'_>,
    (protocol_condition, record_update): (&Expr, &Expr),
) -> Result<bool> {
    let set_state = |value: f64| {
        Stmt::Expr(Expr::LocalSet(arm.state, Box::new(Expr::Number(value))))
    };
    // The record body after its output transfer: `state = 0`, then the
    // user's statements, which read the step value from `output`. Each
    // head binds `output` itself (the protocol Step binds it; on that head
    // the record's transfer keeps it unchanged).
    let mut shared_body = arm.body[1..].to_vec();
    // A binding that is the step value's only reader takes it as a
    // transfer, not as a copy of a local: on the indexed head the value is
    // an element read (already aliased from the array's slot), and the
    // protocol head demotes its Step result itself, as `let x = output`
    // would. Every other body keeps the record's copy.
    let sole_binding = matches!(shared_body.get(1),
        Some(Stmt::Let { init: Some(Expr::LocalGet(value)), .. }) if *value == arm.output)
        && !shared_body[2..].iter().any(|s| stmt_reads_local(s, arm.output));
    if sole_binding {
        if let Some(Stmt::Let { init: Some(init), .. }) = shared_body.get_mut(1) {
            *init = Expr::Sequence(vec![Expr::LocalGet(arm.output)]);
        }
    }
    let shared_body = shared_body.as_slice();
    // The source's declared element type picks the indexed head's length
    // and read lowering, exactly as for `a[i]` in an indexed loop; the
    // read's own guards prove it.
    let element_hint = match arm.body.get(2) {
        Some(Stmt::Let { ty, init: Some(Expr::LocalGet(value)), .. }) if *value == arm.output => {
            ty.clone()
        }
        _ => perry_hir::types::Type::Any,
    };

    // `lower_for` already ran the record's init (the cursor at 0).
    let protocol_mode = crate::lower_conditional::lower_test(ctx, protocol_test)?;
    let head_idx = ctx.new_block("record.head");
    let array_idx = ctx.new_block("record.indexed");
    let array_get_idx = ctx.new_block("record.indexed.get");
    let protocol_idx = ctx.new_block("record.protocol");
    let body_idx = ctx.new_block("record.body");
    let update_idx = ctx.new_block("record.update");
    let exit_idx = ctx.new_block("record.exit");
    let head_label = ctx.block_label(head_idx);
    let array_label = ctx.block_label(array_idx);
    let array_get_label = ctx.block_label(array_get_idx);
    let protocol_label = ctx.block_label(protocol_idx);
    let body_label = ctx.block_label(body_idx);
    let update_label = ctx.block_label(update_idx);
    let exit_label = ctx.block_label(exit_idx);
    ctx.block().br(&head_label);
    ctx.current_block = head_idx;
    ctx.block().cond_br(&protocol_mode, &protocol_label, &array_label);

    // Indexed head. Entry proved the payload an Array on this head. The
    // payload's only other writes are the plan's releases, which run just
    // before an exit leaves the loop, so in this head it is a stable
    // binding of the source's declared element type.
    ctx.current_block = array_idx;
    let previous = (!matches!(element_hint, perry_hir::types::Type::Any)).then(|| {
        let rebound = ctx.reassigned_locals.remove(&arm.array);
        let ty = ctx.local_types.insert(
            arm.array,
            perry_hir::types::Type::Array(Box::new(element_hint)),
        );
        (ty, rebound)
    });
    // An unhinted head keeps the record's proven-array length lowering.
    let condition = if previous.is_some() {
        arm.condition.clone()
    } else {
        Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(Expr::LocalGet(arm.counter)),
            right: Box::new(arm.length.clone()),
        }
    };
    let indexed = (|| -> Result<()> {
        lower_stmt(ctx, &set_state(2.0))?;
        let ready = crate::lower_conditional::lower_test(ctx, &condition)?;
        emit_gc_loop_safepoint(ctx, &[], &[&condition]);
        ctx.block().cond_br(&ready, &array_get_label, &exit_label);
        ctx.current_block = array_get_idx;
        lower_stmt(
            ctx,
            &Stmt::Expr(Expr::LocalSet(arm.output, Box::new(arm.element.clone()))),
        )?;
        if !ctx.block().is_terminated() {
            ctx.block().br(&body_label);
        }
        Ok(())
    })();
    if let Some((ty, rebound)) = previous {
        match ty {
            Some(ty) => ctx.local_types.insert(arm.array, ty),
            None => ctx.local_types.remove(&arm.array),
        };
        if rebound {
            ctx.reassigned_locals.insert(arm.array);
        }
    }
    indexed?;

    // Protocol head: the record's own condition. Step sets state 2, runs
    // IteratorStepValue out of line and binds `output`.
    ctx.current_block = protocol_idx;
    let ready = crate::lower_conditional::lower_test(ctx, protocol_condition)?;
    if sole_binding {
        let value = lower_expr(ctx, &Expr::LocalGet(arm.output))?;
        crate::expr::emit_string_addref_if_heap_string(ctx, &value);
    }
    emit_gc_loop_safepoint(ctx, &[], &[protocol_condition]);
    ctx.block().cond_br(&ready, &body_label, &exit_label);

    // One body. `continue` runs the shared update, `break` leaves through
    // the plan's exits like any loop exit.
    ctx.loop_targets
        .push((update_label.clone(), exit_label.clone(), ctx.try_depth));
    ctx.current_block = body_idx;
    let lowered = lower_stmts(ctx, shared_body);
    ctx.loop_targets.pop();
    lowered?;
    clear_loop_body_shadow_slots(ctx, shared_body);
    if !ctx.block().is_terminated() && body_needs_asm_barrier(shared_body) {
        ctx.block().asm_sideeffect_barrier();
    }
    if !ctx.block().is_terminated() {
        emit_gc_loop_safepoint(ctx, shared_body, &[]);
        ctx.block().br(&update_label);
    }

    // The record's own update: `i++` on the indexed head; the protocol
    // completion never observes the cursor.
    ctx.current_block = update_idx;
    let _ = lower_expr(ctx, record_update)?;
    emit_gc_loop_safepoint(ctx, &[], &[record_update]);
    if !ctx.block().is_terminated() {
        ctx.block().br(&head_label);
    }
    ctx.current_block = exit_idx;
    Ok(true)
}

/// Does `stmt` read local `id`? A closure counts as reading every local:
/// its captures are not part of this expression walk. (Writes are the
/// record's own releases, inlined before each exit.)
fn stmt_reads_local(stmt: &Stmt, id: u32) -> bool {
    fn names(e: &Expr, id: u32) -> bool {
        let mut found = matches!(e, Expr::LocalGet(l) if *l == id)
            || matches!(e, Expr::Closure { .. });
        if !found {
            perry_hir::walker::walk_expr_children(e, &mut |c| found |= names(c, id));
        }
        found
    }
    perry_hir::walker::stmt_any_expr(stmt, &mut |e| names(e, id))
}

fn mode_local(expr: &Expr) -> Option<u32> {
    match expr {
        Expr::Compare {
            op: CompareOp::Eq,
            left,
            right,
        } if matches!(right.as_ref(), Expr::Bool(true)) => match left.as_ref() {
            Expr::LocalGet(id) => Some(*id),
            _ => None,
        },
        _ => None,
    }
}

fn substitute_output(stmt: &mut Stmt, output: u32, element: &Expr) -> bool {
    fn replace_expr(node: &mut Expr, output: u32, element: &Expr) {
        perry_hir::walker::walk_expr_children_mut(node, &mut |e| replace_expr(e, output, element));
        if matches!(node, Expr::LocalGet(id) if *id == output) {
            *node = element.clone();
        }
    }
    match stmt {
        Stmt::Let { init, .. } | Stmt::Return(init) => {
            if let Some(e) = init {
                replace_expr(e, output, element);
            }
            true
        }
        Stmt::Expr(e) => {
            replace_expr(e, output, element);
            true
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            replace_expr(condition, output, element);
            then_branch
                .iter_mut()
                .all(|s| substitute_output(s, output, element))
                && else_branch
                    .as_mut()
                    .is_none_or(|b| b.iter_mut().all(|s| substitute_output(s, output, element)))
        }
        Stmt::Labeled { body, .. } => substitute_output(body, output, element),
        Stmt::Break | Stmt::Continue | Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::types::Type;
    use perry_hir::{BinaryOp, Module, UpdateOp};
    fn rt(method: &str, args: Vec<Expr>) -> Expr {
        Expr::NativeMethodCall {
            module: "__perry_runtime".into(),
            class_name: None,
            object: None,
            method: method.into(),
            args,
        }
    }
    fn local(id: u32, ty: Type, init: Expr) -> Stmt {
        Stmt::Let {
            id,
            name: format!("record_{id}"),
            ty,
            mutable: true,
            init: Some(init),
        }
    }
    #[test]
    fn array_record_literal_bound_is_a_number_not_a_receiver() {
        let ir = emit_record_bound(Type::Array(Box::new(Type::Number)), Some(Expr::Integer(2)));
        assert!(
            ir.lines().any(|line| line.contains("fcmp")
                && line.contains("double")
                && line.ends_with(", 2.0")),
            "literal length is the numeric bound: {ir}"
        );
    }
    fn emit_record(ty: Type) -> String {
        emit_record_bound(ty, None)
    }
    fn emit_record_bound(ty: Type, bound: Option<Expr>) -> String {
        emit_record_payload(ty, bound, false)
    }
    fn emit_record_payload(ty: Type, bound: Option<Expr>, union: bool) -> String {
        emit_record_payload_stack(ty, bound, union, false)
    }
    fn emit_record_payload_stack(
        ty: Type,
        bound: Option<Expr>,
        union: bool,
        stack: bool,
    ) -> String {
        let array_id = if union { 6 } else { 0 };
        let _pin = crate::codegen::helpers::NativeRootsPin::native();
        let mut m = Module::new("record_counted");
        let mode = Expr::Compare {
            op: CompareOp::Eq,
            left: Box::new(Expr::LocalGet(1)),
            right: Box::new(Expr::Bool(true)),
        };
        let update = Expr::Update {
            id: 2,
            op: UpdateOp::Increment,
            prefix: false,
        };
        let candidate =
            matches!(&ty, Type::Array(element) if matches!(element.as_ref(), Type::Number));
        m.init = vec![
            local(
                0,
                ty,
                Expr::Array(vec![Expr::Number(1.0), Expr::Number(2.0)], None),
            ),
            local(
                1,
                Type::Boolean,
                rt("arrayRecordNeedsIterator", {
                    let mut args = vec![Expr::LocalSet(0, Box::new(Expr::LocalGet(0)))];
                    if candidate {
                        args.push(Expr::Bool(true));
                    }
                    args
                }),
            ),
            local(3, Type::Any, Expr::Undefined),
            local(4, Type::Number, Expr::Number(0.0)),
            local(5, Type::Number, Expr::Number(0.0)),
            Stmt::For {
                init: Some(Box::new(local(
                    2,
                    Type::Number,
                    rt("arrayRecordIndex", vec![Expr::Integer(0)]),
                ))),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(2)),
                    right: Box::new(rt(
                        "arrayRecordForBound",
                        vec![
                            mode.clone(),
                            bound.unwrap_or_else(|| {
                                rt("arrayRecordLength", vec![Expr::LocalGet(array_id)])
                            }),
                            Expr::LocalGet(2),
                            rt(
                                "iteratorStep",
                                vec![
                                    Expr::Undefined,
                                    Expr::Undefined,
                                    Expr::LocalSet(3, Box::new(Expr::Undefined)),
                                ],
                            ),
                            Expr::LocalSet(4, Box::new(Expr::Number(2.0))),
                        ],
                    )),
                }),
                update: Some(rt("arrayRecordForUpdate", vec![mode.clone(), update])),
                body: vec![
                    Stmt::Expr(Expr::LocalSet(
                        3,
                        Box::new(rt(
                            "arrayRecordForValue",
                            vec![
                                mode,
                                Expr::LocalGet(3),
                                Expr::IndexGet {
                                    byte_offset: 0,
                                    byte_end: 0,
                                    object: Box::new(Expr::LocalGet(array_id)),
                                    index: Box::new(rt(
                                        "arrayRecordIndex",
                                        vec![Expr::LocalGet(2)],
                                    )),
                                },
                            ],
                        )),
                    )),
                    Stmt::Expr(Expr::LocalSet(4, Box::new(Expr::Number(0.0)))),
                    Stmt::Expr(Expr::LocalSet(
                        5,
                        Box::new(Expr::Binary {
                            op: BinaryOp::Add,
                            left: Box::new(Expr::LocalGet(5)),
                            right: Box::new(Expr::LocalGet(3)),
                        }),
                    )),
                ],
            },
        ];
        if union {
            m.init.insert(
                2,
                local(
                    6,
                    Type::Any,
                    rt(
                        "arrayRecordPayload",
                        vec![
                            Expr::LocalGet(0),
                            Expr::LocalSet(6, Box::new(Expr::LocalGet(0))),
                            Expr::LocalSet(0, Box::new(Expr::Undefined)),
                        ],
                    ),
                ),
            );
        }
        if stack {
            m.init.insert(3, local(7, Type::Any, Expr::Undefined));
            let Stmt::For {
                condition: Some(Expr::Compare { right, .. }),
                ..
            } = m.init.last_mut().unwrap()
            else {
                panic!("record loop")
            };
            let Expr::NativeMethodCall { args, .. } = right.as_mut() else {
                panic!("bound")
            };
            let Expr::NativeMethodCall { args, .. } = &mut args[3] else {
                panic!("step")
            };
            args[0] = Expr::LocalGet(array_id);
            args[1] = Expr::LocalGet(7);
        }
        let ir = String::from_utf8(
            crate::compile_module(
                &m,
                crate::CompileOptions {
                    emit_ir_only: true,
                    is_entry_module: true,
                    target: Some("x86_64-unknown-linux-gnu".into()),
                    ..Default::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
        let start = ir.find("define i32 @main()").unwrap();
        let rest = &ir[start..];
        rest[..rest.find("\n}\n").unwrap()].to_owned()
    }
    #[test]
    fn numeric_stack_record_reads_poll_refreshed_receiver() {
        for ty in [Type::Array(Box::new(Type::Number)), Type::Any] {
            let ir = emit_record_payload_stack(ty, None, true, true);
            assert!(ir.contains("@js_array_record_stack_enter("), "{ir}");
            assert!(!ir.contains("record.capture:"), "entry owns capture: {ir}");
            let body = ir.find("\nfor.packed_f64_fast.body.").expect(&ir);
            let exit = ir[body..].find("\nfor.packed_f64_fast.exit.").expect(&ir);
            let fast = &ir[body..body + exit];
            assert!(
                !fast.contains("load volatile double"),
                "per-element record reload: {ir}"
            );
            assert!(ir.contains("alloca [6 x ptr addrspace(1)]"), "{ir}");
        }
    }

    #[test]
    fn unified_record_payload_carries_the_entry_admission_without_reclassification() {
        let ir = emit_record_payload(Type::Array(Box::new(Type::Number)), None, true);
        assert!(ir.contains("packed_f64.loop.fast.preheader"), "{ir}");
        assert_eq!(
            ir.matches("@js_array_record_enter_counted(").count(),
            1,
            "{ir}"
        );
        assert!(
            !ir.contains("@js_typed_feedback_packed_f64_array_loop_guard("),
            "the single payload must consume its entry admission: {ir}"
        );
        assert_eq!(
            ir.matches("@js_iterator_step(").count(),
            1,
            "one protocol arm: {ir}"
        );
    }

    #[test]
    fn typed_record_uses_counted_admission_and_outlined_protocol() {
        let ir = emit_record(Type::Array(Box::new(Type::Number)));
        assert!(ir.contains("packed_f64.loop.fast.preheader"), "{ir}");
        assert!(ir.contains("@js_array_record_enter_counted("), "{ir}");
        assert!(
            ir.lines()
                .any(|l| l.contains("@js_array_record_enter_counted(") && l.contains("_record")),
            "the entry passes its site's memo word: {ir}"
        );
        assert!(
            !ir.contains("@js_typed_feedback_packed_f64_array_loop_guard("),
            "the loop consumes the entry's admission, not a second classification: {ir}"
        );
        assert_eq!(ir.matches("@js_iterator_step(").count(), 1, "{ir}");
        assert!(
            !ir.contains("0x7FF0000000000000"),
            "protocol consumes done without Infinity: {ir}"
        );
    }
    #[test]
    fn erased_record_reaches_the_same_counted_copy_behind_the_layout_guard() {
        let ir = emit_record(Type::Any);
        assert!(ir.contains("packed_f64.loop.fast.preheader"), "{ir}");
        assert!(ir.contains("@js_array_record_enter("), "{ir}");
        assert!(
            ir.contains("record.indexed.guards.merge"),
            "protocol must bypass indexed guards: {ir}"
        );
        assert!(
            ir.contains("@js_typed_feedback_packed_f64_array_loop_guard("),
            "an erased source proves its layout at the loop: {ir}"
        );
        assert_eq!(ir.matches("@js_iterator_step(").count(), 1, "{ir}");
        assert!(!ir.contains("0x7FF0000000000000"), "{ir}");
    }

    /// #13645: a record whose consumer is not a packed numeric copy steps
    /// its array arm like the indexed loop and its protocol arm through the
    /// record Step, and both heads enter one copy of the body.
    #[test]
    fn array_and_protocol_heads_share_one_body() {
        let _pin = crate::codegen::helpers::NativeRootsPin::native();
        let mut m = Module::new("record_arm");
        let mode = Expr::Compare {
            op: CompareOp::Eq,
            left: Box::new(Expr::LocalGet(1)),
            right: Box::new(Expr::Bool(true)),
        };
        let element = Expr::IndexGet {
            byte_offset: 0,
            byte_end: 0,
            object: Box::new(Expr::LocalGet(0)),
            index: Box::new(rt("arrayRecordIndex", vec![Expr::LocalGet(2)])),
        };
        m.init = vec![
            local(0, Type::Array(Box::new(Type::String)),
                Expr::Array(vec![Expr::String("ab".into()), Expr::String("c".into())], None)),
            local(1, Type::Boolean,
                rt("arrayRecordNeedsIterator", vec![Expr::LocalSet(0, Box::new(Expr::LocalGet(0)))])),
            local(3, Type::Any, Expr::Undefined),
            local(4, Type::Number, Expr::Number(0.0)),
            local(5, Type::Number, Expr::Number(0.0)),
            Stmt::For {
                init: Some(Box::new(local(2, Type::Number, rt("arrayRecordIndex", vec![Expr::Integer(0)])))),
                condition: Some(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(2)),
                    right: Box::new(rt("arrayRecordForBound", vec![
                        mode.clone(),
                        rt("arrayRecordLength", vec![Expr::LocalGet(0)]),
                        Expr::LocalGet(2),
                        rt("iteratorStep", vec![Expr::Undefined, Expr::Undefined,
                            Expr::LocalSet(3, Box::new(Expr::Undefined))]),
                        Expr::LocalSet(4, Box::new(Expr::Number(2.0))),
                    ])),
                }),
                update: Some(rt("arrayRecordForUpdate", vec![mode.clone(),
                    Expr::Update { id: 2, op: UpdateOp::Increment, prefix: false }])),
                body: vec![
                    Stmt::Expr(Expr::LocalSet(3, Box::new(rt("arrayRecordForValue",
                        vec![mode, Expr::LocalGet(3), element])))),
                    Stmt::Expr(Expr::LocalSet(4, Box::new(Expr::Number(0.0)))),
                    Stmt::Let { id: 8, name: "word".into(), ty: Type::String, mutable: false,
                        init: Some(Expr::LocalGet(3)) },
                    Stmt::Expr(Expr::LocalSet(5, Box::new(Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::LocalGet(5)),
                        right: Box::new(Expr::PropertyGet { byte_offset: 0, byte_end: 0,
                            object: Box::new(Expr::LocalGet(8)), property: "length".into() }),
                    }))),
                ],
            },
        ];
        let ir = String::from_utf8(
            crate::compile_module(&m, crate::CompileOptions {
                emit_ir_only: true,
                is_entry_module: true,
                target: Some("x86_64-unknown-linux-gnu".into()),
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        let start = ir.find("define i32 @main()").unwrap();
        let ir = &ir[start..];
        let ir = &ir[..ir.find("\n}\n").unwrap()];
        assert!(ir.contains("\nrecord.indexed."), "{ir}");
        assert!(ir.contains("\nrecord.protocol."), "{ir}");
        assert_eq!(ir.matches("\nrecord.body.").count(), 1, "one body: {ir}");
        assert!(!ir.contains("record.merge."), "no second loop to merge: {ir}");
        assert_eq!(ir.matches("@js_iterator_step(").count(), 1, "one protocol step: {ir}");
    }
}
