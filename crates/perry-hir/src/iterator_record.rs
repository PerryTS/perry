//! Compiler-owned IteratorRecord. One consumer loop and one binding sequence;
//! an ordinary array represents its record as the source plus an integer cursor.
use crate::ir::*;
use crate::lower::LoweringContext;
use crate::types::{LocalId, Type};

pub(crate) struct IteratorRecordPlan {
    use_iter: LocalId,
    source: LocalId,
    array: LocalId,
    index: LocalId,
    scalars: Option<Vec<LocalId>>,
    numeric_candidate: bool,
}

fn runtime(method: &str, args: Vec<Expr>) -> Expr {
    Expr::NativeMethodCall {
        module: "__perry_runtime".into(),
        class_name: None,
        object: None,
        method: method.into(),
        args,
    }
}

impl IteratorRecordPlan {
    pub(crate) fn new(ctx: &mut LoweringContext, source: Expr, out: &mut Vec<Stmt>) -> Self {
        Self::new_typed(ctx, source, Type::Any, out)
    }

    pub(crate) fn new_typed(
        ctx: &mut LoweringContext,
        source: Expr,
        hint: Type,
        out: &mut Vec<Stmt>,
    ) -> Self {
        fn local(
            ctx: &mut LoweringContext,
            out: &mut Vec<Stmt>,
            ty: Type,
            mutable: bool,
            init: Expr,
        ) -> LocalId {
            let id = ctx.fresh_local();
            let name = if mutable && ty == Type::Number {
                format!("__idx_{id}")
            } else {
                format!("__iterator_record_{id}")
            };
            ctx.locals.push((name.clone(), id, ty.clone()));
            out.push(Stmt::Let {
                id,
                name,
                ty,
                mutable,
                init: Some(init),
            });
            id
        }
        // This selects a guarded numeric lowering only. Root release later
        // widens the private source binding to Any, so carry the erased
        // candidate separately from every runtime representation proof.
        let numeric_candidate = matches!(&hint, Type::Array(element)
            if matches!(element.as_ref(), Type::Number));
        let source = local(ctx, out, hint, false, source);
        let use_iter = local(
            ctx,
            out,
            Type::Boolean,
            false,
            runtime(
                "arrayRecordNeedsIterator",
                vec![Expr::LocalSet(source, Box::new(Expr::LocalGet(source)))],
            ),
        );
        // The source already owns the record. Reusing its actual Any value
        // keeps indexed property lowering on runtime facts rather than an
        // artificial declared-array alias, and avoids a second root.
        let array = source;
        // Keep the full u32 cursor as a JS Number. A signed i32 counter would
        // wrap at 2^31; the indexed read consumes its own unsigned proof.
        let index = local(
            ctx,
            out,
            Type::Number,
            true,
            runtime("arrayRecordIndex", vec![Expr::Integer(0)]),
        );
        Self {
            use_iter,
            source,
            array,
            index,
            scalars: None,
            numeric_candidate,
        }
    }

    /// Dense literals have no own iterator member by construction. Evaluate
    /// their elements first, then prove the two prototype members. Keeping the
    /// values in locals avoids creating an array which nobody can observe.
    pub(crate) fn literal(
        ctx: &mut LoweringContext,
        values: Vec<Expr>,
        out: &mut Vec<Stmt>,
    ) -> Self {
        let mut scalars = Vec::with_capacity(values.len());
        for value in values {
            let id = ctx.fresh_local();
            let name = format!("__iterator_literal_{id}");
            ctx.locals.push((name.clone(), id, Type::Any));
            out.push(Stmt::Let {
                id,
                name,
                ty: Type::Any,
                mutable: false,
                init: Some(value),
            });
            scalars.push(id);
        }
        let use_iter = ctx.fresh_local();
        let name = format!("__iterator_record_{use_iter}");
        ctx.locals.push((name.clone(), use_iter, Type::Boolean));
        out.push(Stmt::Let {
            id: use_iter,
            name,
            ty: Type::Boolean,
            mutable: false,
            init: Some(runtime("arrayRecordNeedsIterator", vec![])),
        });
        let source = ctx.fresh_local();
        let name = format!("__iterator_record_{source}");
        ctx.locals.push((name.clone(), source, Type::Any));
        out.push(Stmt::Let {
            id: source,
            name,
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Conditional {
                condition: Box::new(Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(use_iter)),
                    right: Box::new(Expr::Bool(true)),
                }),
                then_expr: Box::new(runtime(
                    "arrayRecordLiteral",
                    scalars.iter().map(|id| Expr::LocalGet(*id)).collect(),
                )),
                else_expr: Box::new(Expr::Undefined),
            }),
        });
        let index = ctx.fresh_local();
        let name = format!("__idx_{index}");
        ctx.locals.push((name.clone(), index, Type::Number));
        out.push(Stmt::Let {
            id: index,
            name,
            ty: Type::Number,
            mutable: true,
            init: Some(runtime("arrayRecordIndex", vec![Expr::Integer(0)])),
        });
        Self {
            use_iter,
            source,
            array: source,
            index,
            scalars: Some(scalars),
            numeric_candidate: false,
        }
    }

    fn length(&self) -> Expr {
        match &self.scalars {
            Some(values) => Expr::Integer(values.len() as i64),
            None => runtime("arrayRecordLength", vec![Expr::LocalGet(self.array)]),
        }
    }

    fn element(&self) -> Expr {
        if let Some(values) = &self.scalars {
            return values
                .iter()
                .enumerate()
                .rev()
                .fold(Expr::Undefined, |otherwise, (i, id)| Expr::Conditional {
                    condition: Box::new(Expr::Compare {
                        op: CompareOp::Eq,
                        left: Box::new(Expr::LocalGet(self.index)),
                        right: Box::new(Expr::Integer(i as i64)),
                    }),
                    then_expr: Box::new(Expr::LocalGet(*id)),
                    else_expr: Box::new(otherwise),
                });
        }
        Expr::IndexGet {
            object: Box::new(Expr::LocalGet(self.array)),
            index: Box::new(runtime(
                "arrayRecordIndex",
                vec![Expr::LocalGet(self.index)],
            )),
        }
    }

    fn close_source(&self) -> Expr {
        if let Some(values) = &self.scalars {
            // Close may expose the original built-in iterator, even when next
            // was replaced after entry. Materialize only for an observable return.
            return Expr::Conditional {
                condition: Box::new(runtime("arrayRecordCloseAbsent", vec![])),
                then_expr: Box::new(Expr::Undefined),
                else_expr: Box::new(runtime(
                    "arrayRecordLiteral",
                    values.iter().map(|id| Expr::LocalGet(*id)).collect(),
                )),
            };
        }
        Expr::LocalGet(self.source)
    }

    pub(crate) fn release(&self, extra: &[LocalId]) -> Vec<Stmt> {
        let mut ids = vec![self.source];
        ids.extend(extra.iter().copied());
        if let Some(scalars) = &self.scalars {
            ids.extend(scalars.iter().copied());
        }
        ids.into_iter()
            .map(|id| Stmt::Expr(Expr::LocalSet(id, Box::new(Expr::Undefined))))
            .collect()
    }

    fn iterator_mode(&self) -> Expr {
        // A constructed predicate carries Boolean evidence without asking the
        // generic bare-binding truthiness path to trust a declaration.
        Expr::Compare {
            op: CompareOp::Eq,
            left: Box::new(Expr::LocalGet(self.use_iter)),
            right: Box::new(Expr::Bool(true)),
        }
    }

    pub(crate) fn guarded_get_iterator(&self) -> Expr {
        Expr::Conditional {
            condition: Box::new(self.iterator_mode()),
            then_expr: Box::new(Expr::GetIterator(Box::new(Expr::LocalGet(self.source)))),
            else_expr: Box::new(Expr::Undefined),
        }
    }

    pub(crate) fn next_init(&self, iter_id: LocalId) -> Expr {
        Expr::Conditional {
            condition: Box::new(self.iterator_mode()),
            then_expr: Box::new(crate::lower::iterator_next_method_call(iter_id)),
            else_expr: Box::new(Expr::Undefined),
        }
    }

    /// Lower IteratorStepValue itself, so every consumer keeps the same
    /// done/failure/close protocol and the same user binding/body.
    pub(crate) fn step(&self, protocol: Expr) -> Expr {
        self.step_at(protocol, None)
    }

    fn step_at(&self, protocol: Expr, literal_index: Option<usize>) -> Expr {
        let Expr::NativeMethodCall { ref args, .. } = protocol else {
            unreachable!()
        };
        let Expr::LocalSet(value_id, _) = args[2] else {
            unreachable!()
        };
        let read_value = !matches!(args.get(3), Some(Expr::Bool(false)));
        if let (Some(values), Some(index)) = (&self.scalars, literal_index) {
            let indexed = if let Some(value) = values.get(index) {
                let mut success = Vec::new();
                if read_value {
                    success.push(Expr::LocalSet(value_id, Box::new(Expr::LocalGet(*value))));
                }
                success.push(Expr::LocalSet(
                    self.index,
                    Box::new(Expr::Integer((index + 1) as i64)),
                ));
                success.push(Expr::Bool(false));
                Expr::Sequence(success)
            } else {
                Expr::Sequence(vec![
                    Expr::LocalSet(value_id, Box::new(Expr::Undefined)),
                    Expr::Bool(true),
                ])
            };
            return Expr::Conditional {
                condition: Box::new(self.iterator_mode()),
                then_expr: Box::new(protocol),
                else_expr: Box::new(indexed),
            };
        }
        let advance = Expr::Update {
            id: self.index,
            op: UpdateOp::Increment,
            prefix: false,
        };
        let element = self.element();
        // Elisions discard the result value, but ArrayIterator.next still
        // performs Get(i), including an inherited getter.
        let mut success = if read_value {
            vec![Expr::LocalSet(value_id, Box::new(element))]
        } else {
            vec![element]
        };
        // The record is private until close. A failed Get marks it done and
        // never closes; after a successful Get, advance before the binding/body
        // can observe the close receiver. The unsigned proof wrapper
        // selects the ordinary indexed backend without narrowing the cursor.
        success.push(advance);
        success.push(Expr::Bool(false));
        Expr::Conditional {
            condition: Box::new(self.iterator_mode()),
            then_expr: Box::new(protocol),
            else_expr: Box::new(Expr::Conditional {
                condition: Box::new(Expr::Compare {
                    op: CompareOp::Lt,
                    left: Box::new(Expr::LocalGet(self.index)),
                    right: Box::new(self.length()),
                }),
                then_expr: Box::new(Expr::Sequence(success)),
                else_expr: Box::new(Expr::Sequence(vec![
                    Expr::LocalSet(value_id, Box::new(Expr::Undefined)),
                    Expr::Bool(true),
                ])),
            }),
        }
    }

    pub(crate) fn rewrite(&self, stmt: &mut Stmt, iter_id: Option<LocalId>, steps: bool) {
        self.rewrite_at(stmt, iter_id, steps, None);
    }

    pub(crate) fn rewrite_at(
        &self,
        stmt: &mut Stmt,
        iter_id: Option<LocalId>,
        steps: bool,
        index: Option<usize>,
    ) {
        let mut edit = |expr: &mut Expr| {
            fn descend(
                plan: &IteratorRecordPlan,
                expr: &mut Expr,
                iter_id: Option<LocalId>,
                steps: bool,
                index: Option<usize>,
            ) {
                crate::walker::walk_expr_children_mut(expr, &mut |e| {
                    descend(plan, e, iter_id, steps, index)
                });
                if let Expr::NativeMethodCall {
                    module,
                    method,
                    args,
                    ..
                } = expr
                {
                    if module != "__perry_runtime" {
                        return;
                    }
                    if steps && method == "iteratorStep" {
                        *expr = plan.step_at(expr.clone(), index);
                    } else if matches!(
                        method.as_str(),
                        "iteratorCloseIfNotDone" | "iteratorCloseOnThrow"
                    ) && iter_id.is_none_or(
                        |id| matches!(args.first(), Some(Expr::LocalGet(i)) if *i == id),
                    ) {
                        let throwing = method == "iteratorCloseOnThrow";
                        let array_close = runtime(
                            "arrayRecordClose",
                            vec![
                                plan.close_source(),
                                Expr::LocalGet(plan.index),
                                args[1].clone(),
                                if throwing {
                                    args[2].clone()
                                } else {
                                    Expr::Undefined
                                },
                                Expr::Bool(throwing),
                            ],
                        );
                        *expr = Expr::Conditional {
                            condition: Box::new(plan.iterator_mode()),
                            then_expr: Box::new(expr.clone()),
                            else_expr: Box::new(array_close),
                        };
                    }
                }
            }
            descend(self, expr, iter_id, steps, index);
        };
        walk_stmt(stmt, &mut edit);
    }

    pub(crate) fn drive(
        &self,
        ctx: &mut LoweringContext,
        iter_id: LocalId,
        value_id: LocalId,
        mut body: Vec<Stmt>,
    ) -> Vec<Stmt> {
        for stmt in &mut body {
            self.rewrite(stmt, Some(iter_id), false);
            walk_stmt(stmt, &mut |expr| {
                fn cursor_after_get(expr: &mut Expr) {
                    crate::walker::walk_expr_children_mut(expr, &mut cursor_after_get);
                    if let Expr::NativeMethodCall {
                        module,
                        method,
                        args,
                        ..
                    } = expr
                    {
                        if module == "__perry_runtime" && method == "arrayRecordClose" {
                            args[1] = Expr::Binary {
                                op: BinaryOp::Add,
                                left: Box::new(args[1].clone()),
                                right: Box::new(Expr::Integer(1)),
                            };
                        }
                    }
                }
                cursor_after_get(expr);
            });
        }
        let next = ctx.fresh_local();
        let name = format!("__iterator_next_{next}");
        ctx.locals.push((name.clone(), next, Type::Any));
        // One completion handler owns the loop. Next failures and exhaustion
        // mark it closed before advancing; successful steps reopen body close.
        // This preserves IteratorClose ordering without enter/leave per step.
        let Some(Stmt::Try {
            body: mut user,
            catch,
            mut finally,
        }) = body.pop()
        else {
            unreachable!("record loop requires its completion handler");
        };
        let state_init = user.remove(0);
        let Stmt::Let { id: state, .. } = &state_init else {
            unreachable!()
        };
        let state = *state;
        body.extend(user);
        // One counted consumer. On an override the bound advances the
        // captured protocol out of line; on a proven array it reads live length.
        // Keep close disabled during Step/Get, then enable it for the user body.
        let bound = runtime(
            "arrayRecordForBound",
            vec![
                self.iterator_mode(),
                Expr::LocalGet(self.array),
                Expr::LocalGet(self.index),
                crate::lower::iterator_step_call(iter_id, next, value_id),
                Expr::LocalSet(state, Box::new(Expr::Number(2.0))),
                Expr::Bool(self.numeric_candidate),
            ],
        );
        let mut stepped_body = vec![
            Stmt::Expr(Expr::LocalSet(
                value_id,
                Box::new(runtime(
                    "arrayRecordForValue",
                    vec![
                        self.iterator_mode(),
                        Expr::LocalGet(value_id),
                        self.element(),
                    ],
                )),
            )),
            Stmt::Expr(Expr::LocalSet(state, Box::new(Expr::Number(0.0)))),
        ];
        stepped_body.extend(body);
        finally.get_or_insert_with(Vec::new).push(Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Ne,
                left: Box::new(Expr::LocalGet(state)),
                right: Box::new(Expr::Number(0.0)),
            },
            then_branch: self.release(&[iter_id, next, value_id]),
            else_branch: None,
        });
        vec![
            Stmt::Let {
                id: next,
                name,
                ty: Type::Any,
                mutable: false,
                init: Some(self.next_init(iter_id)),
            },
            Stmt::Let {
                id: value_id,
                name: format!("__iterator_value_{value_id}"),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Undefined),
            },
            state_init,
            Stmt::Try {
                body: vec![Stmt::For {
                    init: Some(Box::new(Stmt::Let {
                        id: self.index,
                        name: format!("__idx_{}", self.index),
                        ty: Type::Number,
                        mutable: true,
                        init: Some(runtime("arrayRecordIndex", vec![Expr::Integer(0)])),
                    })),
                    condition: Some(Expr::Compare {
                        op: CompareOp::Lt,
                        left: Box::new(Expr::LocalGet(self.index)),
                        right: Box::new(bound),
                    }),
                    update: Some(runtime(
                        "arrayRecordForUpdate",
                        vec![
                            self.iterator_mode(),
                            Expr::Update {
                                id: self.index,
                                op: UpdateOp::Increment,
                                prefix: false,
                            },
                        ],
                    )),
                    body: stepped_body,
                }],
                catch,
                finally,
            },
        ]
    }
}

fn walk_stmt(stmt: &mut Stmt, f: &mut impl FnMut(&mut Expr)) {
    match stmt {
        Stmt::Let { init, .. } | Stmt::Return(init) => {
            if let Some(e) = init {
                f(e);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => f(e),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            f(condition);
            for s in then_branch {
                walk_stmt(s, f);
            }
            if let Some(body) = else_branch {
                for s in body {
                    walk_stmt(s, f);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
            f(condition);
            for s in body {
                walk_stmt(s, f);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(s) = init {
                walk_stmt(s, f);
            }
            if let Some(e) = condition {
                f(e);
            }
            if let Some(e) = update {
                f(e);
            }
            for s in body {
                walk_stmt(s, f);
            }
        }
        Stmt::Labeled { body, .. } => walk_stmt(body, f),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                walk_stmt(s, f);
            }
            if let Some(c) = catch {
                for s in &mut c.body {
                    walk_stmt(s, f);
                }
            }
            if let Some(body) = finally {
                for s in body {
                    walk_stmt(s, f);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            f(discriminant);
            for c in cases {
                if let Some(e) = &mut c.test {
                    f(e);
                }
                for s in &mut c.body {
                    walk_stmt(s, f);
                }
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_record_materializes_only_for_protocol_or_observable_close() {
        let mut ctx = LoweringContext::new("literal.ts");
        let mut setup = Vec::new();
        let plan = IteratorRecordPlan::literal(
            &mut ctx,
            vec![Expr::Integer(1), Expr::Integer(2)],
            &mut setup,
        );
        assert_eq!(plan.scalars.as_ref().unwrap().len(), 2);
        assert!(
            matches!(&setup[2], Stmt::Let { init: Some(Expr::NativeMethodCall { method, args, .. }), .. }
            if method == "arrayRecordNeedsIterator" && args.is_empty())
        );
        assert!(
            matches!(&setup[3], Stmt::Let { init: Some(Expr::Conditional { then_expr, else_expr, .. }), .. }
            if matches!(then_expr.as_ref(), Expr::NativeMethodCall { method, args, .. } if method == "arrayRecordLiteral" && args.len() == 2)
                && matches!(else_expr.as_ref(), Expr::Undefined))
        );
        assert!(
            matches!(plan.close_source(), Expr::Conditional { condition, else_expr, .. }
            if matches!(condition.as_ref(), Expr::NativeMethodCall { method, .. } if method == "arrayRecordCloseAbsent")
                && matches!(else_expr.as_ref(), Expr::NativeMethodCall { method, args, .. } if method == "arrayRecordLiteral" && args.len() == 2))
        );
        assert_eq!(
            plan.release(&[99, 100]).len(),
            5,
            "private source, step bindings, and scalar owners must be cleared"
        );
    }

    #[test]
    fn array_record_guards_allocations_and_reads_live_length() {
        let mut ctx = LoweringContext::new("array_record.ts");
        let mut setup = Vec::new();
        let plan = IteratorRecordPlan::new(&mut ctx, Expr::Array(vec![]), &mut setup);
        assert_eq!(
            plan.array, plan.source,
            "the indexed record must reuse the source rather than a declared-array alias"
        );
        assert_eq!(setup.len(), 3, "one source, one mode, one cursor");
        assert!(setup.iter().any(|stmt| matches!(stmt,
            Stmt::Let { id, ty: Type::Number, init: Some(Expr::NativeMethodCall { method, args, .. }), .. }
            if *id == plan.index && method == "arrayRecordIndex"
                && matches!(args.as_slice(), [Expr::Integer(0)]))),
            "the cursor initializer must not acquire a signed i32 shadow");
        assert!(matches!(plan.guarded_get_iterator(), Expr::Conditional {
            condition, then_expr, else_expr,
        } if matches!(condition.as_ref(), Expr::Compare { op: CompareOp::Eq, left, right }
                if matches!(left.as_ref(), Expr::LocalGet(id) if *id == plan.use_iter)
                    && matches!(right.as_ref(), Expr::Bool(true)))
            && matches!(*then_expr, Expr::GetIterator(_))
            && matches!(*else_expr, Expr::Undefined)));
        assert!(
            matches!(plan.next_init(99), Expr::Conditional { condition, then_expr, .. }
            if matches!(condition.as_ref(), Expr::Compare { op: CompareOp::Eq, left, right }
                if matches!(left.as_ref(), Expr::LocalGet(id) if *id == plan.use_iter)
                    && matches!(right.as_ref(), Expr::Bool(true)))
            && matches!(then_expr.as_ref(), Expr::NativeMethodCall { method, .. } if method == "iteratorNextMethod"))
        );
        let Expr::Conditional { else_expr, .. } =
            plan.step(crate::lower::iterator_step_call(99, 100, 101))
        else {
            panic!()
        };
        let Expr::Conditional {
            condition,
            then_expr,
            ..
        } = *else_expr
        else {
            panic!()
        };
        let Expr::Compare { right, .. } = *condition else {
            panic!()
        };
        assert!(
            matches!(*right, Expr::NativeMethodCall { module, method, args, .. }
            if module == "__perry_runtime" && method == "arrayRecordLength" && args.len() == 1),
            "each step must read live length"
        );
        let Expr::Sequence(success) = *then_expr else {
            panic!()
        };
        assert!(
            matches!(&success[0], Expr::LocalSet(101, element) if matches!(element.as_ref(),
            Expr::IndexGet { index, .. } if matches!(index.as_ref(),
                Expr::NativeMethodCall { method, .. } if method == "arrayRecordIndex")))
        );
        assert!(
            matches!(&success[1], Expr::Update { id, op: UpdateOp::Increment, .. }
            if *id == plan.index),
            "advance exactly once before the consumer binding"
        );
    }

    fn hir(source: &str) -> String {
        let source = source.to_owned();
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(move || {
                let mut cache = perry_diagnostics::SourceCache::new();
                let parsed = perry_parser::parse_typescript_with_cache(
                    &source,
                    "array_record.ts",
                    &mut cache,
                )
                .unwrap();
                format!(
                    "{:?}",
                    crate::lower_module(&parsed.module, "test", "array_record.ts").unwrap()
                )
            })
            .unwrap()
            .join()
            .unwrap()
    }

    #[test]
    fn array_record_one_body_and_shape_entry() {
        for source in [
            "const a = [1,2,3]; for (const x of a) console.log('unique-body', x);",
            "function f(a: any) { for (const x of a) console.log('unique-body', x); }",
            "function f(a: number[]) { for (const x of a) console.log('unique-body', x); }",
        ] {
            let ir = hir(source);
            assert!(ir.contains("arrayRecordNeedsIterator"), "{ir}");
            assert!(
                ir.contains("iteratorStep") && ir.contains("IndexGet"),
                "{ir}"
            );
            assert_eq!(ir.matches("unique-body").count(), 1, "{ir}");
            assert!(!ir.contains("ArrayIterationPatched"), "{ir}");
            assert!(ir.contains("arrayRecordClose"), "{ir}");
        }
    }

    #[test]
    fn literal_binding_step_preserves_exact_close_cursor_without_indexing() {
        let mut ctx = LoweringContext::new("literal");
        let mut setup = Vec::new();
        let plan = IteratorRecordPlan::literal(
            &mut ctx,
            vec![Expr::Integer(10), Expr::Integer(20)],
            &mut setup,
        );
        let first = plan.step_at(crate::lower::iterator_step_call(99, 100, 101), Some(0));
        let Expr::Conditional { else_expr, .. } = first else {
            panic!()
        };
        let Expr::Sequence(success) = *else_expr else {
            panic!()
        };
        assert!(matches!(&success[0], Expr::LocalSet(101, value)
            if matches!(value.as_ref(), Expr::LocalGet(id) if *id == plan.scalars.as_ref().unwrap()[0])));
        assert!(matches!(&success[1], Expr::LocalSet(id, value)
            if *id == plan.index && matches!(value.as_ref(), Expr::Integer(1))));
        let exhausted = plan.step_at(crate::lower::iterator_step_call(99, 100, 101), Some(2));
        assert!(!format!("{exhausted:?}").contains("IndexGet"));
        assert!(matches!(exhausted, Expr::Conditional { else_expr, .. }
            if matches!(else_expr.as_ref(), Expr::Sequence(body)
                if matches!(body.last(), Some(Expr::Bool(true))))));
    }

    #[test]
    fn array_record_typed_candidate_survives_private_root_release() {
        let mut ctx = LoweringContext::new("hint");
        let mut setup = Vec::new();
        let typed = IteratorRecordPlan::new_typed(
            &mut ctx,
            Expr::Undefined,
            Type::Array(Box::new(Type::Number)),
            &mut setup,
        );
        assert!(typed.numeric_candidate);
        let general = IteratorRecordPlan::new(&mut ctx, Expr::Undefined, &mut setup);
        assert!(!general.numeric_candidate);
        let ir = hir("function f(a: number[]) { for (const x of a) { console.log(x); } }");
        assert!(
            ir.contains("Number(2.0)), Bool(true)"),
            "static candidate survives source release: {ir}"
        );
    }

    #[test]
    fn array_record_counted_step_preserves_continue_and_close_state() {
        let mut ctx = LoweringContext::new("array_record.ts");
        let mut setup = Vec::new();
        let plan = IteratorRecordPlan::new(&mut ctx, Expr::Array(vec![]), &mut setup);
        let state = ctx.fresh_local();
        let lowered = plan.drive(
            &mut ctx,
            99,
            100,
            vec![Stmt::Try {
                body: vec![
                    Stmt::Let {
                        id: state,
                        name: "completion".into(),
                        ty: Type::Number,
                        mutable: true,
                        init: Some(Expr::Number(0.0)),
                    },
                    Stmt::Continue,
                ],
                catch: None,
                finally: None,
            }],
        );
        let Stmt::Try { body: outer, .. } = &lowered[3] else {
            panic!()
        };
        let Stmt::For {
            condition, body, ..
        } = &outer[0]
        else {
            panic!()
        };
        assert!(
            matches!(condition, Some(Expr::Compare { op: CompareOp::Lt, right, .. })
            if matches!(right.as_ref(), Expr::NativeMethodCall { method, args, .. }
                if method == "arrayRecordForBound"
                    && matches!(&args[4], Expr::LocalSet(id, value)
                        if *id == state && matches!(value.as_ref(), Expr::Number(2.0))))),
            "Step disables close before both protocol advance and live-length exhaustion"
        );
        assert!(matches!(&body[0], Stmt::Expr(Expr::LocalSet(100, value))
            if matches!(value.as_ref(), Expr::NativeMethodCall { method, .. }
                if method == "arrayRecordForValue")));
        assert!(matches!(&body[1], Stmt::Expr(Expr::LocalSet(id, value))
            if *id == state && matches!(value.as_ref(), Expr::Number(0.0))));
        assert!(
            matches!(&body[2], Stmt::Continue),
            "continue executes the counted update and the next captured step"
        );
        assert_eq!(
            format!("{lowered:?}")
                .matches("method: \"iteratorStep\"")
                .count(),
            1
        );
    }

    #[test]
    fn array_record_rest_empty_and_nested_share_the_step() {
        let rest = hir("function f(a: any) { const [x,...xs] = a; return xs; }");
        assert!(
            rest.contains("iteratorRestAppend")
                && rest.contains("IndexGet")
                && rest.contains("iteratorStep"),
            "{rest}"
        );
        assert!(!rest.contains("iteratorRestToArray"), "{rest}");
        let empty = hir("const [] = [];");
        assert!(
            empty.contains("arrayRecordNeedsIterator") && empty.contains("arrayRecordClose"),
            "{empty}"
        );
        let nested = hir("const [[x,y]] = [[1,2]];");
        assert_eq!(
            nested.matches("arrayRecordNeedsIterator").count(),
            2,
            "{nested}"
        );
    }

    #[test]
    fn array_record_destructure_uses_same_step_and_done() {
        let ir = hir("function f(a: any) { const [x,y,z] = a; return [x,y,z]; }");
        assert_eq!(ir.matches("arrayRecordNeedsIterator").count(), 1, "{ir}");
        assert_eq!(ir.matches("method: \"iteratorStep\"").count(), 3, "{ir}");
        assert!(
            ir.contains("IndexGet") && ir.contains("arrayRecordClose"),
            "{ir}"
        );
        assert!(!ir.contains("ArrayIterationPatched"), "{ir}");
    }
}
