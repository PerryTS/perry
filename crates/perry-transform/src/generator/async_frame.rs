//! An async activation's owner is its queued step closure, never its creator's
//! native frame. The step is retained by AsyncFirstCall, by the scanned FIFO
//! AsyncStep payload, or by a pending reaction's traced closure capture. The
//! runner roots that allocation before entering any collecting context hook.
use super::*;
use std::collections::{HashMap, HashSet};

fn each(stmts: &[Stmt], f: &mut impl FnMut(&Expr)) {
    fn expr(e: &Expr, f: &mut impl FnMut(&Expr)) {
        f(e);
        if let Expr::Closure { body, .. } = e {
            each(body, f);
        }
        perry_hir::walker::walk_expr_children(e, &mut |e| expr(e, f));
    }
    for s in stmts {
        perry_hir::walker::stmt_any_expr(s, &mut |e| {
            expr(e, f);
            false
        });
    }
}

/// References and consumption are counted independently. A function value,
/// alias, publication, returned promise or stored promise cannot pass merely
/// because a different call is directly awaited. No annotation is consulted.
pub(super) fn admission(module: &Module) -> HashSet<FuncId> {
    if !module.imports.is_empty()
        || !module.exports.is_empty()
        || !module.classes.is_empty()
        || perry_hir::has_deferred_dynamic_code_sites()
        || module
            .class_family_syntax
            .as_ref()
            .is_none_or(|s| s.module_capability_uses != 0)
    {
        return HashSet::new();
    }
    let functions: HashMap<_, _> = module.functions.iter().map(|f| (f.id, f)).collect();
    let mut references = HashMap::<FuncId, usize>::new();
    let mut consumed = HashMap::<FuncId, usize>::new();
    let mut local_uses = HashMap::<LocalId, usize>::new();
    let mut local_awaits = HashMap::<LocalId, usize>::new();
    let mut scan = |e: &Expr| {
        match e {
            Expr::LocalGet(id) | Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
                *local_uses.entry(*id).or_default() += 1;
            }
            Expr::Closure {
                captures,
                mutable_captures,
                ..
            } => {
                for id in captures.iter().chain(mutable_captures) {
                    *local_uses.entry(*id).or_default() += 1;
                }
            }
            _ => {}
        }
        if let Expr::FuncRef(id) = e {
            *references.entry(*id).or_default() += 1;
        }
        if let Expr::Yield {
            value: Some(value),
            delegate: false,
        }
        | Expr::Await(value) = e
        {
            if let Expr::LocalGet(id) = value.as_ref() {
                *local_awaits.entry(*id).or_default() += 1;
            }
            if let Expr::Call { callee, .. } = value.as_ref() {
                if let Expr::FuncRef(id) = callee.as_ref() {
                    *consumed.entry(*id).or_default() += 1;
                }
            }
        }
    };
    each(&module.init, &mut scan);
    for f in &module.functions {
        each(&f.body, &mut scan);
    }
    drop(scan);
    for (_, id) in &module.script_global_functions {
        *references.entry(*id).or_default() += 1;
    }
    for f in &module.functions {
        for id in &f.captures {
            *local_uses.entry(*id).or_default() += 1;
        }
    }
    // Async lowering hoists call operands into immutable locals before the
    // Yield. Admit those values only when the complete reference census is
    // direct awaits: aliases, writes, captures and publication add unmatched
    // references. The original-source capability census rules out hidden
    // eval/loader uses; no compiler-private variable spelling is a proof.
    let mut births = HashMap::<LocalId, Vec<FuncId>>::new();
    fn holders(stmts: &mut [Stmt], out: &mut HashMap<LocalId, Vec<FuncId>>) {
        for s in stmts {
            if let Stmt::Let {
                id,
                mutable: false,
                init: Some(Expr::Call { callee, .. }),
                ..
            } = s
            {
                if let Expr::FuncRef(fid) = callee.as_ref() {
                    out.entry(*id).or_default().push(*fid);
                }
            }
            for b in crate::closure_local_inline::nested_stmt_lists(s) {
                holders(b, out)
            }
            crate::closure_local_inline::for_each_expr_in_stmt_mut(s, &mut |e| {
                if let Expr::Closure { body, .. } = e {
                    holders(body, out)
                }
            });
        }
    }
    holders(&mut module.init.clone(), &mut births);
    for f in &module.functions {
        holders(&mut f.body.clone(), &mut births);
    }
    for (id, targets) in births {
        if targets.len() == 1
            && local_awaits.get(&id).copied().unwrap_or(0) > 0
            && local_uses.get(&id) == local_awaits.get(&id)
        {
            *consumed.entry(targets[0]).or_default() += 1;
        }
    }
    // Discarding the promise does not publish it. Rejections and all queue
    // ordering remain handled by the existing async machinery.
    for s in &module.init {
        if let Stmt::Expr(Expr::Call { callee, .. }) = s {
            if let Expr::FuncRef(id) = callee.as_ref() {
                *consumed.entry(*id).or_default() += 1;
            }
        }
    }
    functions
        .values()
        .filter(|f| {
            f.was_plain_async
                && f.is_strict
                && !f.is_exported
                && f.captures.is_empty()
                && f.params
                    .iter()
                    .all(|p| !p.is_rest && p.default.is_none() && p.arguments_object.is_none())
                && references.get(&f.id).copied().unwrap_or(0) > 0
                && references.get(&f.id) == consumed.get(&f.id)
                && closed_body(&f.body, &functions, &mut HashSet::new())
        })
        .map(|f| f.id)
        .collect()
}

// A closed callback/effect graph, not a callee annotation. No nested closure
// can observe an activation binding. Strict caller boundaries prevent
// reflective callee access. Every value stays a generic traced JSValue;
// operand conversions and Promise assimilation retain their full effects.
// Intrinsic Promise operations retain their full
// runtime validation/assimilation and the ordinary queue.
fn closed_body(
    stmts: &[Stmt],
    functions: &HashMap<FuncId, &Function>,
    visiting: &mut HashSet<FuncId>,
) -> bool {
    let mut safe = true;
    each(stmts, &mut |e| {
        safe &= match e {
            Expr::Undefined
            | Expr::Null
            | Expr::Bool(_)
            | Expr::Number(_)
            | Expr::Integer(_)
            | Expr::String(_)
            | Expr::LocalGet(_)
            | Expr::LocalSet(..)
            | Expr::Update { .. }
            | Expr::Binary { .. }
            | Expr::Unary { .. }
            | Expr::Compare { .. }
            | Expr::Logical { .. }
            | Expr::Conditional { .. }
            | Expr::Sequence(_)
            | Expr::FuncRef(_)
            | Expr::Yield {
                delegate: false, ..
            }
            | Expr::Await(_) => true,
            Expr::Call { callee, .. } => {
                if promise_intrinsic(callee) {
                    true
                } else if let Expr::FuncRef(id) = callee.as_ref() {
                    if visiting.contains(id) {
                        true
                    } else if let Some(f) = functions.get(id) {
                        visiting.insert(*id);
                        let ok = f.is_strict
                            && f.captures.is_empty()
                            && closed_body(&f.body, functions, visiting);
                        visiting.remove(id);
                        ok
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            Expr::GlobalGet(0) => true,
            Expr::PropertyGet {
                object, property, ..
            } => {
                (property == "Promise" && matches!(object.as_ref(), Expr::GlobalGet(0)))
                    || promise_intrinsic(e)
            }
            Expr::NativeMethodCall {
                module,
                class_name: None,
                object: None,
                method,
                ..
            } if module == "Promise" && matches!(method.as_str(), "resolve" | "reject") => true,
            _ => false,
        };
    });
    safe
}

pub(super) fn lower(f: &mut Function, controls: (Vec<LocalId>, Vec<LocalId>)) {
    let Some(Stmt::Return(Some(Expr::AsyncFirstCall { step_closure }))) = f.body.last() else {
        return;
    };
    let Expr::LocalGet(step) = step_closure.as_ref() else {
        return;
    };
    let step = *step;
    let Some(index) = f.body.iter().position(
        |s| matches!(s, Stmt::Let { id, init: Some(Expr::Closure { .. }), .. } if *id == step),
    ) else {
        return;
    };
    let mut declared: HashSet<_> = collect_hoisted_vars(&f.body[..index])
        .iter()
        .map(|v| v.0)
        .collect();
    declared.extend(f.params.iter().map(|p| p.id));
    let Stmt::Let {
        init: Some(closure),
        ..
    } = &mut f.body[index]
    else {
        unreachable!()
    };
    let Expr::Closure {
        captures,
        mutable_captures,
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        ..
    } = closure
    else {
        return;
    };
    if captures.iter().any(|id| !declared.contains(id)) {
        return;
    }
    let slots: HashSet<_> = captures.iter().copied().collect();
    mutable_captures.clear();
    if let Expr::Closure { body, .. } = closure {
        fn releases(stmts: &mut [Stmt], slots: &HashSet<u32>) {
            for s in stmts {
                if let Stmt::ReleaseBoxes(ids) = s {
                    ids.retain(|id| !slots.contains(id));
                }
                for b in crate::closure_local_inline::nested_stmt_lists(s) {
                    releases(b, slots)
                }
            }
        }
        releases(body, &slots);
    }
    let owner = std::mem::replace(closure, Expr::Undefined);
    *closure = Expr::NativeMethodCall {
        module: "__perry_runtime".into(),
        class_name: None,
        object: None,
        method: "privateAsyncResumeFrame".into(),
        args: vec![
            owner,
            Expr::Sequence(controls.0.into_iter().map(Expr::LocalGet).collect()),
            Expr::Sequence(controls.1.into_iter().map(Expr::LocalGet).collect()),
        ],
    };
    for s in &mut f.body {
        if let Stmt::PreallocateBoxes(ids) = s {
            ids.retain(|id| !slots.contains(id));
        }
    }
}

fn promise_intrinsic(e: &Expr) -> bool {
    matches!(e, Expr::PropertyGet {object, property, ..}
        if matches!(property.as_str(), "resolve" | "reject")
        && matches!(object.as_ref(), Expr::PropertyGet {object, property, ..}
            if property == "Promise" && matches!(object.as_ref(), Expr::GlobalGet(0))))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Module {
        let mut m = Module::new("async_owner");
        m.class_family_syntax = Some(Default::default());
        m.functions.push(Function {
            id: 0,
            name: "suspended".into(),
            type_params: vec![],
            params: vec![],
            return_type: Type::Any,
            body: vec![
                Stmt::Expr(Expr::Yield {
                    value: Some(Box::new(Expr::Number(7.0))),
                    delegate: false,
                }),
                Stmt::Return(Some(Expr::Number(8.0))),
            ],
            is_async: false,
            is_generator: true,
            is_strict: true,
            is_exported: false,
            captures: vec![],
            decorators: vec![],
            was_plain_async: true,
            was_unrolled: false,
        });
        m.init = vec![Stmt::Expr(Expr::Call {
            byte_end: 0,
            callee: Box::new(Expr::FuncRef(0)),
            args: vec![],
            type_args: vec![],
            byte_offset: 0,
        })];
        m
    }
    fn admitted(m: &Module) -> bool {
        format!("{:?}", m.functions).contains("method: \"privateAsyncResumeFrame\"")
    }
    #[test]
    fn queued_async_owner_has_inline_slots_without_creator_stack_or_annotations() {
        for ty in [Type::Any, Type::Number, Type::String] {
            let mut m = fixture();
            m.functions[0].return_type = ty;
            assert!(admission(&m).contains(&0));
            super::super::transform_generators(&mut m);
            assert!(admitted(&m), "known queue-owned async activation refused");
            assert!(format!("{:?}", m.functions).contains("AsyncFirstCall"));
            assert!(format!("{:?}", m.functions).contains("AsyncStepChain"));
            for s in &m.functions[0].body {
                if let Stmt::PreallocateBoxes(ids) = s {
                    assert!(
                        ids.is_empty(),
                        "private activation retained standalone cells"
                    );
                }
            }
        }
    }
    #[test]
    fn settled_continuation_loop_is_emitted_only_for_plain_async() {
        let mut plain = fixture();
        super::super::transform_generators(&mut plain);
        let text = format!("{:?}", plain.functions);
        assert!(
            text.contains("AsyncStepResumeNow"),
            "plain async lacks the SNC probe"
        );
        assert!(
            text.contains("AsyncStepChain"),
            "decline path must keep the FIFO publication"
        );
        assert!(
            text.contains("While"),
            "SNC continues through a loop, never recursion"
        );
        let mut generator = fixture();
        generator.functions[0].was_plain_async = false;
        super::super::transform_generators(&mut generator);
        let text = format!("{:?}", generator.functions);
        assert!(
            !text.contains("AsyncStepResumeNow"),
            "a user generator must not get SNC"
        );
    }
    #[test]
    fn async_control_descriptor_carries_exact_transform_created_ids() {
        let mut m = fixture();
        super::super::transform_generators(&mut m);
        let declared: HashSet<_> = collect_hoisted_vars(&m.functions[0].body)
            .into_iter()
            .map(|v| v.0)
            .collect();
        let mut checked = 0;
        each(&m.functions[0].body, &mut |e| {
            if let Expr::NativeMethodCall {
                module,
                method,
                args,
                ..
            } = e
            {
                if module == "__perry_runtime" && method == "privateAsyncResumeFrame" {
                    let [_, Expr::Sequence(numbers), Expr::Sequence(booleans)] = args.as_slice()
                    else {
                        panic!("private async control descriptor absent")
                    };
                    assert_eq!(numbers.len(), 1, "private async state fact lost");
                    assert_eq!(booleans.len(), 2, "private async boolean facts lost");
                    assert!(numbers
                        .iter()
                        .chain(booleans)
                        .all(|e| matches!(e,Expr::LocalGet(id) if declared.contains(id))));
                    checked += 1;
                }
            }
        });
        assert_eq!(checked, 1);
    }

    #[test]
    fn stored_or_returned_promise_and_function_alias_refuse_private_async() {
        for alias in [false, true] {
            let mut m = fixture();
            if alias {
                m.init.push(Stmt::Let {
                    id: 90,
                    name: "alias".into(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::FuncRef(0)),
                });
            } else {
                let Stmt::Expr(call) = m.init.pop().unwrap() else {
                    unreachable!()
                };
                m.init.push(Stmt::Return(Some(call)));
            }
            super::super::transform_generators(&mut m);
            assert!(
                !admitted(&m),
                "escaped promise or callback lost its standalone activation"
            );
        }
    }
    #[test]
    fn shared_capture_unknown_callback_or_nonstrict_caller_refuse_private_async() {
        for mode in 0..4 {
            let mut m = fixture();
            match mode {
                0 => m.functions[0].captures.push(91),
                1 => {
                    m.functions[0].params.push(perry_hir::Param {
                        id: 91,
                        name: "callback".into(),
                        ty: Type::Any,
                        is_rest: false,
                        default: None,
                        decorators: vec![],
                        arguments_object: None,
                    });
                    m.functions[0].body.push(Stmt::Expr(Expr::Call {
                        byte_end: 0,
                        callee: Box::new(Expr::LocalGet(91)),
                        args: vec![],
                        type_args: vec![],
                        byte_offset: 0,
                    }));
                }
                2 => m.functions[0].is_strict = false,
                _ => m.functions[0].body.push(Stmt::Expr(Expr::Closure {
                    func_id: 9,
                    params: vec![],
                    return_type: Type::Any,
                    body: vec![Stmt::Return(Some(Expr::LocalGet(91)))],
                    captures: vec![91],
                    mutable_captures: vec![],
                    captures_this: false,
                    captures_new_target: false,
                    enclosing_class: None,
                    is_arrow: true,
                    is_async: false,
                    is_generator: false,
                    is_strict: true,
                })),
            }
            super::super::transform_generators(&mut m);
            assert!(
                !admitted(&m),
                "unproven capture/callback/effect became private"
            );
        }
    }
    #[test]
    fn inline_async_slots_must_originate_in_the_constructed_wrapper() {
        let mut m = fixture();
        let mut next_local = 100;
        let mut next_func = 100;
        super::super::transform_generator_function(
            &mut m.functions[0],
            &mut next_local,
            &mut next_func,
        );
        let mut planted = false;
        for s in &mut m.functions[0].body {
            if let Stmt::Let {
                init: Some(Expr::Closure { captures, .. }),
                ..
            } = s
            {
                captures.push(91);
                planted = true;
            }
        }
        assert!(planted);
        lower(&mut m.functions[0], (vec![], vec![]));
        assert!(
            !admitted(&m),
            "a foreign capture has no inline ownership proof"
        );
    }
    #[test]
    fn hoisted_async_promise_must_have_only_await_uses_and_closed_source() {
        for mode in 0..4 {
            let mut m = fixture();
            let Stmt::Expr(call) = m.init.pop().unwrap() else {
                unreachable!()
            };
            m.init = vec![
                Stmt::Let {
                    id: 90,
                    name: "arbitrary_source_name".into(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(call),
                },
                Stmt::Expr(Expr::Await(Box::new(Expr::LocalGet(90)))),
            ];
            match mode {
                1 => m.init.push(Stmt::Return(Some(Expr::LocalGet(90)))),
                2 => m.init.push(Stmt::Let {
                    id: 91,
                    name: "alias".into(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::LocalGet(90)),
                }),
                3 => {
                    m.class_family_syntax
                        .as_mut()
                        .unwrap()
                        .module_capability_uses = 1
                }
                _ => {}
            }
            super::super::transform_generators(&mut m);
            assert_eq!(
                admitted(&m),
                mode == 0,
                "await-only promise or source capability proof was lost"
            );
        }
    }
}
