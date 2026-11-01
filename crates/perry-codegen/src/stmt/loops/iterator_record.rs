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
                    init: &candidate_init,
                    condition: &candidate_condition,
                    update: &updates[1],
                    array,
                    counter: *counter,
                    output: *output,
                    state: *state,
                    length: &args[1],
                    element: &values[2],
                    body,
                },
                (init, condition.unwrap(), update.unwrap(), body),
            );
        }
    }
    if !was_nonnegative {
        ctx.nonnegative_integer_locals.remove(counter);
    }
    result
}

struct ArrayArm<'a> {
    init: &'a Stmt,
    condition: &'a Expr,
    update: &'a Expr,
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
/// below the live length (ECMA-262 23.1.5.1). That arm gets its own copy of
/// the body, lowered by the ordinary counted-loop chain, so the binding and
/// the body lower like `for (let i = 0; i < a.length; i++) { const x = a[i]; }`.
/// The protocol arm keeps the record loop unchanged. Both arms share the
/// cursor, completion state and output locals, so close, abrupt completion
/// and release see the same record either way.
fn lower_array_arm(
    ctx: &mut FnCtx<'_>,
    protocol_test: &Expr,
    arm: ArrayArm<'_>,
    protocol: (Option<&Stmt>, &Expr, &Expr, &[Stmt]),
) -> Result<bool> {
    let set_state = |value: f64| {
        Stmt::Expr(Expr::LocalSet(arm.state, Box::new(Expr::Number(value))))
    };
    // Completion state 2 while Get runs: a throwing getter on a hole does not
    // close (IteratorStepValue errors complete the record). The record body
    // then sets 0 before the user statements.
    let user = &arm.body[2..];
    // A binding whose only input is the step value reads the element itself:
    // the declared element type of the source picks its lowering, exactly as
    // for `const x = a[i]`; the read's own guards prove it.
    let binding = match user.first() {
        Some(Stmt::Let { id, name, ty, mutable, init: Some(Expr::LocalGet(value)) })
            if *value == arm.output && !user[1..].iter().any(|s| stmt_reads_local(s, arm.output)) =>
        {
            Some((Stmt::Let { id: *id, name: name.clone(), ty: ty.clone(), mutable: *mutable, init: Some(arm.element.clone()) }, ty.clone()))
        }
        _ => None,
    };
    let mut array_body = vec![set_state(2.0)];
    let element_hint = match binding {
        Some((stmt, ty)) => {
            array_body.push(stmt);
            array_body.push(set_state(0.0));
            array_body.extend(user[1..].iter().cloned());
            ty
        }
        None => {
            array_body.push(Stmt::Expr(Expr::LocalSet(arm.output, Box::new(arm.element.clone()))));
            array_body.extend(arm.body[1..].iter().cloned());
            perry_hir::types::Type::Any
        }
    };

    let protocol_mode = crate::lower_conditional::lower_test(ctx, protocol_test)?;
    let protocol_idx = ctx.new_block("record.protocol");
    let array_idx = ctx.new_block("record.indexed");
    let merge_idx = ctx.new_block("record.merge");
    let protocol_label = ctx.block_label(protocol_idx);
    let array_label = ctx.block_label(array_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&protocol_mode, &protocol_label, &array_label);

    ctx.current_block = array_idx;
    // Entry proved the payload an Array on this arm; its element hint is
    // the source's declared one, as for an indexed loop over that source.
    // The payload's only other writes are the plan's releases, which run
    // just before an exit leaves this arm, so here it is a stable binding.
    let previous = (!matches!(element_hint, perry_hir::types::Type::Any)).then(|| {
        let rebound = ctx.reassigned_locals.remove(&arm.array);
        let ty = ctx.local_types.insert(
            arm.array,
            perry_hir::types::Type::Array(Box::new(element_hint)),
        );
        (ty, rebound)
    });
    // An unhinted arm keeps the record's proven-array length lowering.
    let condition = if previous.is_some() {
        arm.condition.clone()
    } else {
        Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(Expr::LocalGet(arm.counter)),
            right: Box::new(arm.length.clone()),
        }
    };
    lower_for_tiers(ctx, Some(arm.init), Some(&condition), Some(arm.update), &array_body)?;
    if let Some((ty, rebound)) = previous {
        match ty {
            Some(ty) => ctx.local_types.insert(arm.array, ty),
            None => ctx.local_types.remove(&arm.array),
        };
        if rebound {
            ctx.reassigned_locals.insert(arm.array);
        }
    }
    if !ctx.block().is_terminated() {
        // Exhaustion completes the record (state 2) for its release; a break
        // left its own state behind.
        lower_stmt(
            ctx,
            &Stmt::If {
                condition: Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(arm.state)),
                    right: Box::new(Expr::Number(0.0)),
                },
                then_branch: vec![set_state(2.0)],
                else_branch: None,
            },
        )?;
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_label);
        }
    }

    ctx.current_block = protocol_idx;
    lower_for_after_init(ctx, protocol.0, Some(protocol.1), Some(protocol.2), protocol.3, "for")?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }
    ctx.current_block = merge_idx;
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

    /// #13645: a record whose consumer is not a packed numeric copy still
    /// gives the array arm its own indexed loop; the binding reads the
    /// element and the protocol arm keeps the record step.
    #[test]
    fn array_arm_is_an_indexed_loop_beside_the_protocol_record() {
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
        let arm = ir.find("\nrecord.indexed.").expect(ir);
        let protocol = ir.find("\nrecord.protocol.").expect(ir);
        assert!(ir.contains("record.merge."), "{ir}");
        assert_eq!(ir.matches("@js_iterator_step(").count(), 1, "one protocol step: {ir}");
        assert!(arm != protocol, "{ir}");
    }
}
