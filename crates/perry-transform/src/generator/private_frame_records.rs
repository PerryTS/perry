//! Identity transport through the compiler's array-or-iterator record ABI.
//! A constructed canonical generator takes only the iterator arm. We can
//! remove that record's private aliases only when their entire reference and
//! write census fits the exact intrinsic contract; source names/types do not
//! participate. The resulting child references must still pass protocol_only.
use super::*;

fn rt<'a>(e: &'a Expr, method: &str) -> Option<&'a [Expr]> {
    match e {
        Expr::NativeMethodCall {
            module,
            class_name: None,
            object: None,
            method: m,
            args,
        } if module == "__perry_runtime" && m == method => Some(args),
        _ => None,
    }
}
fn get(e: &Expr, id: u32) -> bool {
    matches!(e,Expr::LocalGet(other) if *other==id)
}
fn mode(e: &Expr, id: u32) -> bool {
    matches!(e,Expr::Compare {op:CompareOp::Eq,left,right} if get(left,id) && matches!(right.as_ref(),Expr::Bool(true)))
}
fn payload_shape(e: &Expr, record: u32, flag: u32) -> Option<u32> {
    let a = rt(e, "arrayRecordPayload")?;
    let [source, Expr::LocalSet(payload, value), Expr::LocalSet(clear, undefined)] = a else {
        return None;
    };
    if !get(source, record) || *clear != record || !matches!(undefined.as_ref(), Expr::Undefined) {
        return None;
    }
    let Expr::Conditional {
        condition,
        then_expr,
        else_expr,
    } = value.as_ref()
    else {
        return None;
    };
    (mode(condition, flag)
        && matches!(then_expr.as_ref(),Expr::GetIterator(input) if get(input,record))
        && get(else_expr, record))
    .then_some(*payload)
}

pub(super) fn normalize(stmts: &[Stmt], child: u32) -> Vec<Stmt> {
    use super::private_frame::{reference_counts, visit_stmts};
    let mut declarations = Vec::new();
    fn declarations_in(stmts: &mut [Stmt], child: u32, out: &mut Vec<u32>) {
        for s in stmts {
            if let Stmt::Let {
                id,
                init: Some(Expr::LocalGet(input)),
                ..
            } = s
            {
                if *input == child {
                    out.push(*id)
                }
            }
            for b in crate::closure_local_inline::nested_stmt_lists(s) {
                declarations_in(b, child, out)
            }
        }
    }
    declarations_in(&mut stmts.to_vec(), child, &mut declarations);
    let counts = reference_counts(stmts);
    let mut records = Vec::new();
    for record in declarations {
        let mut flags = Vec::new();
        visit_stmts(stmts, &mut |e| {
            if let Expr::LocalSet(flag, value) = e {
                if let Some([Expr::LocalSet(id, value)]) = rt(value, "arrayRecordNeedsIterator") {
                    if *id == record && get(value, record) {
                        flags.push(*flag)
                    }
                }
            }
        });
        if flags.len() != 1 || counts.get(&record).copied() != Some(6) {
            continue;
        }
        let flag = flags[0];
        let mut payloads = Vec::new();
        let mut flag_writes = 0;
        let mut bad = false;
        visit_stmts(stmts, &mut |e| {
            if let Some(payload) = payload_shape(e, record, flag) {
                payloads.push(payload)
            }
            if let Expr::LocalSet(id, _) = e {
                if *id == flag {
                    flag_writes += 1
                }
            }
            if matches!(e,Expr::Update {id,..} if *id==flag) {
                bad = true
            }
        });
        if bad || flag_writes != 1 || payloads.len() != 1 {
            continue;
        }
        let payload = payloads[0];
        visit_stmts(stmts, &mut |e| {
            if let Expr::LocalSet(id, value) = e {
                if *id == payload
                    && !matches!(value.as_ref(), Expr::Undefined)
                    && payload_shape(value, record, flag) != Some(payload)
                    && !matches!(value.as_ref(),Expr::Conditional {condition,then_expr,else_expr}
                        if mode(condition,flag) && matches!(then_expr.as_ref(),Expr::GetIterator(input) if get(input,record)) && get(else_expr,record))
                {
                    bad = true
                }
            }
            if matches!(e,Expr::Update {id,..} if *id==payload) {
                bad = true
            }
        });
        if !bad {
            records.push((record, flag, payload))
        }
    }
    let mut result = stmts.to_vec();
    for (record, flag, payload) in records {
        fn rewrite(e: &mut Expr, child: u32, record: u32, flag: u32, payload: u32) {
            if get(e, flag) {
                *e = Expr::Bool(true);
                return;
            }
            if get(e, payload) {
                *e = Expr::LocalGet(child);
                return;
            }
            if rt(e, "arrayRecordNeedsIterator").is_some_and(
                |a| matches!(a,[Expr::LocalSet(id,value)] if *id==record && get(value,record)),
            ) {
                *e = Expr::Bool(true);
                return;
            }
            if payload_shape(e, record, flag) == Some(payload) {
                *e = Expr::Undefined;
                return;
            }
            // These intrinsics evaluate only the selected arm. Discard the
            // array arm before walking it, exactly as the codegen ABI does.
            if let Expr::NativeMethodCall {
                module,
                method,
                args,
                ..
            } = e
            {
                if module == "__perry_runtime" && args.first().is_some_and(|e| mode(e, flag)) {
                    if method == "arrayRecordForBound" && args.len() == 5 {
                        args[1] = Expr::Undefined;
                    }
                    if method == "arrayRecordForValue" && args.len() == 3 {
                        args[2] = Expr::Undefined;
                    }
                }
            }
            perry_hir::walker::walk_expr_children_mut(e, &mut |e| {
                rewrite(e, child, record, flag, payload)
            });
            if let Expr::Compare {
                op: CompareOp::Eq,
                left,
                right,
            } = e
            {
                if let (Expr::Bool(a), Expr::Bool(b)) = (left.as_ref(), right.as_ref()) {
                    *e = Expr::Bool(a == b);
                    return;
                }
            }
            if let Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } = e
            {
                if let Expr::Bool(yes) = condition.as_ref() {
                    *e = if *yes {
                        then_expr.as_ref().clone()
                    } else {
                        else_expr.as_ref().clone()
                    };
                }
            }
        }
        fn list(stmts: &mut [Stmt], child: u32, record: u32, flag: u32, payload: u32) {
            for s in stmts {
                if let Stmt::Let { id, init, .. } = s {
                    if *id == record {
                        *init = Some(Expr::Undefined)
                    }
                }
                crate::closure_local_inline::for_each_expr_in_stmt_mut(s, &mut |e| {
                    rewrite(e, child, record, flag, payload)
                });
                for b in crate::closure_local_inline::nested_stmt_lists(s) {
                    list(b, child, record, flag, payload)
                }
            }
        }
        list(&mut result, child, record, flag, payload);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn intrinsic(method: &str, args: Vec<Expr>) -> Expr {
        Expr::NativeMethodCall {
            module: "__perry_runtime".into(),
            class_name: None,
            object: None,
            method: method.into(),
            args,
        }
    }
    fn fixture() -> Vec<Stmt> {
        let mode = Expr::Compare {
            op: CompareOp::Eq,
            left: Box::new(Expr::LocalGet(21)),
            right: Box::new(Expr::Bool(true)),
        };
        vec![
            Stmt::Let {
                id: 20,
                name: "source_alias".into(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::LocalGet(10)),
            },
            Stmt::Expr(Expr::LocalSet(
                21,
                Box::new(intrinsic(
                    "arrayRecordNeedsIterator",
                    vec![Expr::LocalSet(20, Box::new(Expr::LocalGet(20)))],
                )),
            )),
            Stmt::Expr(Expr::LocalSet(
                22,
                Box::new(intrinsic(
                    "arrayRecordPayload",
                    vec![
                        Expr::LocalGet(20),
                        Expr::LocalSet(
                            22,
                            Box::new(Expr::Conditional {
                                condition: Box::new(mode),
                                then_expr: Box::new(Expr::GetIterator(Box::new(Expr::LocalGet(
                                    20,
                                )))),
                                else_expr: Box::new(Expr::LocalGet(20)),
                            }),
                        ),
                        Expr::LocalSet(20, Box::new(Expr::Undefined)),
                    ],
                )),
            )),
            Stmt::Expr(intrinsic(
                "iteratorStep",
                vec![
                    Expr::LocalGet(22),
                    Expr::Undefined,
                    Expr::LocalSet(23, Box::new(intrinsic("iteratorStepOutput", vec![]))),
                ],
            )),
        ]
    }
    #[test]
    fn record_transport_requires_sealed_aliases_mode_and_payload_writes() {
        for mode in 0..4 {
            let mut body = fixture();
            match mode {
                1 => body.push(Stmt::Return(Some(Expr::LocalGet(20)))),
                2 => body.push(Stmt::Expr(Expr::LocalSet(21, Box::new(Expr::Bool(false))))),
                3 => body.push(Stmt::Expr(Expr::LocalSet(22, Box::new(Expr::LocalGet(90))))),
                _ => {}
            }
            let normalized = normalize(&body, 10);
            assert_eq!(
                super::super::private_frame::protocol_only(&normalized, 10),
                mode == 0,
                "record identity transport lost its sealed-use proof"
            );
        }
    }
}
