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
    if module != "__perry_runtime"
        || method != "arrayRecordForBound"
        || args.len() != 6
        || !matches!(args[5], Expr::Bool(true))
    {
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
            object: Box::new(Expr::LocalGet(array)),
            property: "length".into(),
            byte_offset: 0,
        }),
    };
    let element = Expr::IndexGet {
        object: Box::new(Expr::LocalGet(array)),
        index: Box::new(Expr::LocalGet(*counter)),
    };
    let mut candidate_body = body[2..].to_vec();
    // Successful packed admission proves this read pure and numeric. The
    // ordinary matcher must independently prove that every consumer statement
    // is call-free and cannot mutate the receiver/length. Only that native
    // copy can substitute the transfer; the protocol keeps its output slot.
    for stmt in &mut candidate_body {
        if !substitute_output(stmt, *output, &element) {
            return Ok(false);
        }
    }
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
    let result = lower_packed_f64_versioned_for_with_protocol(
        ctx,
        Some(&candidate_init),
        Some(&candidate_condition),
        Some(&updates[1]),
        &candidate_body,
        Some((&values[0], condition.unwrap(), update.unwrap(), body)),
    );
    if !was_nonnegative {
        ctx.nonnegative_integer_locals.remove(counter);
    }
    result
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
                Expr::Array(vec![Expr::Number(1.0), Expr::Number(2.0)]),
            ),
            local(
                1,
                Type::Boolean,
                rt(
                    "arrayRecordNeedsIterator",
                    vec![Expr::LocalSet(0, Box::new(Expr::LocalGet(0)))],
                ),
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
                                rt("arrayRecordLength", vec![Expr::LocalGet(0)])
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
                            Expr::Bool(candidate),
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
                                    object: Box::new(Expr::LocalGet(0)),
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
    fn typed_record_uses_counted_admission_and_outlined_protocol() {
        let ir = emit_record(Type::Array(Box::new(Type::Number)));
        assert!(ir.contains("packed_f64.loop.fast.preheader"), "{ir}");
        assert!(ir.contains("@js_array_record_enter"), "{ir}");
        assert!(
            ir.contains("record.indexed.guards.merge"),
            "protocol must bypass indexed guards: {ir}"
        );
        assert!(
            ir.contains("@js_typed_feedback_packed_f64_array_loop_guard"),
            "{ir}"
        );
        assert_eq!(ir.matches("@js_iterator_step(").count(), 1, "{ir}");
        assert!(
            !ir.contains("0x7FF0000000000000"),
            "protocol consumes done without Infinity: {ir}"
        );
    }
    #[test]
    fn general_record_keeps_the_shared_protocol_without_numeric_assumption() {
        let ir = emit_record(Type::Any);
        assert!(!ir.contains("packed_f64.loop.fast.preheader"), "{ir}");
        assert_eq!(ir.matches("@js_iterator_step(").count(), 1, "{ir}");
        assert!(!ir.contains("0x7FF0000000000000"), "{ir}");
    }
}
