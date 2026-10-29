//! Private resume frames for closed, protocol-only local consumers.
//!
//! This first admission is deliberately closed: no imports/exports/classes,
//! reflection, property mutation, opaque callees or foreign inputs. These are
//! HIR value/use/effect facts; no source type participates in the proof. The
//! ordinary wrapper remains available at every escaping or unproven site.
use super::*;
use std::collections::{HashMap, HashSet};

fn runtime(method: &str, args: Vec<Expr>) -> Expr {
    Expr::NativeMethodCall {
        module: "__perry_runtime".into(),
        class_name: None,
        object: None,
        method: method.into(),
        args,
    }
}

fn visit_expr(expr: &Expr, f: &mut impl FnMut(&Expr)) {
    f(expr);
    if let Expr::Closure { body, .. } = expr {
        visit_stmts(body, f);
    }
    perry_hir::walker::walk_expr_children(expr, &mut |e| visit_expr(e, f));
}
pub(super) fn visit_stmts(stmts: &[Stmt], f: &mut impl FnMut(&Expr)) {
    for stmt in stmts {
        perry_hir::walker::stmt_any_expr(stmt, &mut |expr| {
            visit_expr(expr, f);
            false
        });
    }
}

// Primitive output is a construction fact, including on every successful
// arithmetic result. It never consults a declared TypeScript type.
fn primitive_output(expr: &Expr, globals: &HashSet<LocalId>) -> bool {
    match expr {
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::String(_)
        | Expr::Binary { .. }
        | Expr::Unary { .. }
        | Expr::Compare { .. } => true,
        Expr::LocalGet(id) => globals.contains(id),
        Expr::LocalSet(_, value) => primitive_output(value, globals),
        Expr::Logical { left, right, .. } => {
            primitive_output(left, globals) && primitive_output(right, globals)
        }
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => primitive_output(then_expr, globals) && primitive_output(else_expr, globals),
        Expr::Sequence(exprs) => exprs.last().is_some_and(|e| primitive_output(e, globals)),
        _ => false,
    }
}
fn primitive_globals(module: &Module) -> HashSet<LocalId> {
    let empty = HashSet::new();
    let mut known = HashSet::new();
    let mut bad = HashSet::new();
    for stmt in &module.init {
        if let Stmt::Let { id, init, .. } = stmt {
            if init.as_ref().is_none_or(|e| primitive_output(e, &empty)) {
                known.insert(*id);
            } else {
                bad.insert(*id);
            }
        }
    }
    let mut writes = |e: &Expr| match e {
        Expr::LocalSet(id, value) if !primitive_output(value, &empty) => {
            bad.insert(*id);
        }
        Expr::Closure { params, .. } => bad.extend(params.iter().map(|p| p.id)),
        _ => {}
    };
    visit_stmts(&module.init, &mut writes);
    for f in &module.functions {
        visit_stmts(&f.body, &mut writes);
    }
    drop(writes);
    for f in &module.functions {
        bad.extend(f.params.iter().map(|p| p.id));
    }
    known.retain(|id| !bad.contains(id));
    known
}

/// A finite closed target set for each callback parameter. Unknown actuals
/// poison that parameter, even if another call supplies a known closure.
fn closed_effects(module: &Module) -> bool {
    if !module.imports.is_empty() || !module.exports.is_empty() || !module.classes.is_empty() {
        return false;
    }
    let functions: HashMap<_, _> = module.functions.iter().map(|f| (f.id, f)).collect();
    let mut targets = HashSet::new();
    let mut unknown = HashSet::new();
    let mut calls = |e: &Expr| {
        if let Expr::Call { callee, args, .. } = e {
            if let Expr::FuncRef(fid) = callee.as_ref() {
                if let Some(f) = functions.get(fid) {
                    for (index, p) in f.params.iter().enumerate() {
                        match args.get(index) {
                            Some(Expr::Closure {
                                is_async: false,
                                is_generator: false,
                                ..
                            }) => {
                                targets.insert(p.id);
                            }
                            Some(Expr::FuncRef(fid)) if functions.contains_key(fid) => {
                                targets.insert(p.id);
                            }
                            _ => {
                                unknown.insert(p.id);
                            }
                        }
                    }
                }
            }
        }
    };
    visit_stmts(&module.init, &mut calls);
    for f in &module.functions {
        visit_stmts(&f.body, &mut calls);
    }
    let mut writes = |e: &Expr| {
        if let Expr::LocalSet(id, _) | Expr::Update { id, .. } = e {
            unknown.insert(*id);
        }
    };
    visit_stmts(&module.init, &mut writes);
    for f in &module.functions {
        visit_stmts(&f.body, &mut writes);
    }
    let mut self_steps = HashSet::new();
    fn self_ids(stmts: &mut [Stmt], out: &mut HashSet<u32>) {
        for s in stmts {
            if let Stmt::Let {
                id,
                init: Some(Expr::CurrentStepClosure),
                ..
            } = s
            {
                out.insert(*id);
            }
            for b in crate::closure_local_inline::nested_stmt_lists(s) {
                self_ids(b, out);
            }
            crate::closure_local_inline::for_each_expr_in_stmt_mut(s, &mut |e| {
                if let Expr::Closure { body, .. } = e {
                    self_ids(body, out);
                }
            });
        }
    }
    self_ids(&mut module.init.clone(), &mut self_steps);
    for f in &module.functions {
        self_ids(&mut f.body.clone(), &mut self_steps);
    }
    let primitives = primitive_globals(module);
    let mut safe = true;
    let mut check = |e: &Expr| {
        let allowed = match e {
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
            | Expr::GetIterator(_)
            | Expr::CurrentStepClosure
            | Expr::AsyncFirstCall { .. }
            | Expr::AsyncStepChain { .. }
            | Expr::AsyncStepResumeNow { .. }
            | Expr::AsyncStepDeclined(..)
            | Expr::AsyncStepDone { .. }
            | Expr::IterResultSet(..)
            | Expr::IterResultGetValue
            | Expr::IterResultGetDone
            | Expr::GlobalGet(0)
            | Expr::LinkGeneratorPrototype {
                fixed_state: true,
                is_async: false,
                ..
            }
            | Expr::Closure {
                is_async: false,
                is_generator: false,
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                ..
            } => true,
            Expr::IndexGet { index, .. } => matches!(index.as_ref(), Expr::NativeMethodCall {
                module,class_name:None,object:None,method,.. } if module=="__perry_runtime" && method=="arrayRecordIndex"),
            Expr::PropertyGet {
                object, property, ..
            } => {
                matches!(object.as_ref(), Expr::GlobalGet(0))
                    && matches!(property.as_str(), "log" | "reject")
            }
            Expr::Call { callee, args, .. } => match callee.as_ref() {
                Expr::FuncRef(fid) => functions.contains_key(fid),
                Expr::LocalGet(id) => {
                    (targets.contains(id) && !unknown.contains(id)) || self_steps.contains(id)
                }
                Expr::PropertyGet {
                    object, property, ..
                } if matches!(object.as_ref(), Expr::GlobalGet(0)) => {
                    property == "reject"
                        || (property == "log"
                            && args.iter().all(|a| primitive_output(a, &primitives)))
                }
                _ => false,
            },
            Expr::NativeMethodCall {
                module,
                class_name: None,
                object: None,
                method,
                ..
            } if module == "__perry_runtime" => matches!(
                method.as_str(),
                "privateResumeFrame"
                    | "privateResumeFrameStep"
                    | "privateResumeFrameClose"
                    | "privateResumeFrameCloseOnThrow"
                    | "privateAsyncResumeFrame"
                    | "iteratorNextMethod"
                    | "iteratorStep"
                    | "iteratorStepOutput"
                    | "iteratorCloseIfNotDone"
                    | "iteratorCloseOnThrow"
                    | "arrayRecordNeedsIterator"
                    | "arrayRecordIndex"
                    | "arrayRecordPayload"
                    | "arrayRecordForBound"
                    | "arrayRecordLength"
                    | "arrayRecordForValue"
                    | "arrayRecordForUpdate"
                    | "arrayRecordClose"
            ),
            _ => false,
        };
        safe &= allowed;
    };
    // Terminal logging is allowed only for primitive values. Its operands
    // still participate in the effect proof: a late getter or Error/Function
    // inspection can expose effects captured while a private frame was live.
    let terminal = match module.init.last() {
        Some(Stmt::Expr(Expr::Call { callee, args, .. }))
            if matches!(callee.as_ref(),
            Expr::PropertyGet {object,property,..} if property=="log" && matches!(object.as_ref(),Expr::GlobalGet(_))) =>
        {
            Some(args)
        }
        _ => None,
    };
    let init = if let Some(args) = terminal {
        let primitives = primitive_globals(module);
        if !args.iter().all(|arg| primitive_output(arg, &primitives)) {
            return false;
        }
        for arg in args {
            visit_expr(arg, &mut check);
        }
        &module.init[..module.init.len() - 1]
    } else {
        &module.init
    };
    visit_stmts(init, &mut check);
    for f in &module.functions {
        visit_stmts(&f.body, &mut check);
    }
    safe
}

/// Every reference must be the receiver of a compiler-owned protocol op.
/// Explicitly reject captures (including closures whose bodies do not read
/// the capture) and writes. A type annotation cannot relax this check.
pub(super) fn protocol_only(stmts: &[Stmt], id: LocalId) -> bool {
    fn expr(e: &Expr, id: LocalId) -> bool {
        match e {
            Expr::LocalGet(other) => *other != id,
            Expr::LocalSet(other, _) | Expr::Update { id: other, .. } if *other == id => false,
            Expr::NativeMethodCall {
                module,
                method,
                args,
                ..
            } if module == "__perry_runtime" && method == "privateResumeFrame" => {
                matches!(args.first(), Some(Expr::LinkGeneratorPrototype {obj, fixed_state:true, is_async:false, ..})
                    if matches!(obj.as_ref(), Expr::Closure {body, mutable_captures, ..}
                        if mutable_captures.is_empty() && protocol_only(&super::private_frame_records::normalize(body,id), id)))
            }
            Expr::Closure {
                captures,
                mutable_captures,
                body,
                ..
            } => {
                !captures.contains(&id)
                    && !mutable_captures.contains(&id)
                    && protocol_only(body, id)
            }
            Expr::NativeMethodCall {
                module,
                object: None,
                class_name: None,
                method,
                args,
            } if module == "__perry_runtime"
                && matches!(
                    method.as_str(),
                    "iteratorNextMethod"
                        | "iteratorStep"
                        | "iteratorCloseIfNotDone"
                        | "iteratorCloseOnThrow"
                )
                && matches!(args.first(), Some(Expr::LocalGet(other)) if *other == id) =>
            {
                args.iter().skip(1).all(|e| expr(e, id))
            }
            _ => {
                let mut safe = true;
                perry_hir::walker::walk_expr_children(e, &mut |e| safe &= expr(e, id));
                safe
            }
        }
    }
    !stmts
        .iter()
        .any(|s| perry_hir::walker::stmt_any_expr(s, &mut |e| !expr(e, id)))
}

fn nested_closure(stmts: &[Stmt]) -> bool {
    let mut found = false;
    visit_stmts(stmts, &mut |e| found |= matches!(e, Expr::Closure { .. }));
    found
}
pub(super) fn run(module: &mut Module) {
    if !closed_effects(module) {
        return;
    }
    let templates: HashMap<_, _> = module.functions.iter().filter(|f| {
        !f.is_exported && f.params.iter().all(|p| !p.is_rest && p.default.is_none() && p.arguments_object.is_none())
        && f.body.iter().all(|s| !matches!(s, Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) if !ids.is_empty()))
        && matches!(f.body.last(), Some(Stmt::Return(Some(Expr::LinkGeneratorPrototype {
            fixed_state: true, is_async: false, obj, .. }))) if matches!(obj.as_ref(), Expr::Closure {
                mutable_captures, body, captures_this: false, captures_new_target: false, enclosing_class: None, .. } if mutable_captures.is_empty() && !nested_closure(body)))
    }).map(|f| (f.id, f.clone())).collect();
    let mut next_local = compute_max_local_id(module) + 1;
    let mut next_func = compute_max_func_id(module) + 1;
    let mut uses = HashMap::<u32, usize>::new();
    let mut count = |e: &Expr| match e {
        Expr::LocalGet(id) | Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
            *uses.entry(*id).or_default() += 1
        }
        Expr::Closure {
            captures,
            mutable_captures,
            ..
        } => {
            for id in captures.iter().chain(mutable_captures.iter()) {
                *uses.entry(*id).or_default() += 1;
            }
        }
        _ => {}
    };
    visit_stmts(&module.init, &mut count);
    for f in &module.functions {
        visit_stmts(&f.body, &mut count);
    }
    drop(count);
    for f in &module.functions {
        for id in &f.captures {
            *uses.entry(*id).or_default() += 1;
        }
    }
    lower_list(
        &mut module.init,
        &templates,
        &uses,
        &mut next_local,
        &mut next_func,
    );
    for f in &mut module.functions {
        lower_owned_resume(f, &templates, &uses, &mut next_local, &mut next_func);
    }
}

fn lower_list(
    stmts: &mut Vec<Stmt>,
    templates: &HashMap<FuncId, Function>,
    uses: &HashMap<u32, usize>,
    next_local: &mut u32,
    next_func: &mut u32,
) {
    for stmt in stmts.iter_mut() {
        for body in crate::closure_local_inline::nested_stmt_lists(stmt) {
            lower_list(body, templates, uses, next_local, next_func);
        }
    }
    let mut at = 0;
    while at < stmts.len() {
        let assigned = matches!(&stmts[at], Stmt::Expr(Expr::LocalSet(_, _)));
        let normalized = match &stmts[at] {
            Stmt::Expr(Expr::LocalSet(id, value)) => Stmt::Let {
                id: *id,
                name: "__owned_child".into(),
                ty: Type::Any,
                mutable: true,
                init: Some(value.as_ref().clone()),
            },
            s => s.clone(),
        };
        let candidate = match &normalized {
            Stmt::Let {
                id,
                init: Some(Expr::Call { callee, args, .. }),
                ..
            } => match callee.as_ref() {
                Expr::FuncRef(fid) => templates
                    .get(fid)
                    .filter(|f| f.params.len() == args.len())
                    .map(|f| (*id, f.clone(), args.clone())),
                _ => None,
            },
            _ => None,
        };
        let Some((id, f, args)) = candidate else {
            at += 1;
            continue;
        };
        let mut scoped_uses = 0;
        visit_stmts(&stmts[at + 1..], &mut |e| match e {
            Expr::LocalGet(other) | Expr::LocalSet(other, _) | Expr::Update { id: other, .. }
                if *other == id =>
            {
                scoped_uses += 1
            }
            Expr::Closure {
                captures,
                mutable_captures,
                ..
            } => {
                scoped_uses += captures
                    .iter()
                    .chain(mutable_captures)
                    .filter(|other| **other == id)
                    .count()
            }
            _ => {}
        });
        if scoped_uses
            != uses
                .get(&id)
                .copied()
                .unwrap_or(0)
                .saturating_sub(usize::from(assigned))
            || !protocol_only(&stmts[at + 1..], id)
        {
            at += 1;
            continue;
        }
        let mut setup = f.body.clone();
        let Some(Stmt::Return(Some(link))) = setup.pop() else {
            unreachable!()
        };
        let Expr::LinkGeneratorPrototype { obj, .. } = &link else {
            unreachable!()
        };
        let Expr::Closure { captures, .. } = obj.as_ref() else {
            unreachable!()
        };
        // A source/global/shared binding is not private storage. Every child
        // slot must originate in this exact wrapper's parameters/declarations.
        let mut declared: HashSet<_> = collect_hoisted_vars(&setup).iter().map(|v| v.0).collect();
        declared.extend(f.params.iter().map(|p| p.id));
        if captures.iter().any(|id| !declared.contains(id)) {
            at += 1;
            continue;
        }
        let mut locals = declared;
        visit_expr(&link, &mut |e| {
            if let Expr::Closure { params, .. } = e {
                locals.extend(params.iter().map(|p| p.id));
            }
        });
        // Includes catch bindings and one-state declarations inside the resume.
        fn ids(stmts: &[Stmt], out: &mut HashSet<u32>) {
            for s in stmts {
                match s {
                    Stmt::Let { id, .. } => {
                        out.insert(*id);
                    }
                    Stmt::Try { catch: Some(c), .. } => {
                        if let Some((id, _)) = &c.param {
                            out.insert(*id);
                        }
                    }
                    _ => {}
                }
                perry_hir::walker::stmt_any_expr(s, &mut |e| {
                    if let Expr::Closure { body, .. } = e {
                        ids(body, out);
                    }
                    false
                });
                // collect_hoisted_vars includes nested statement declarations.
            }
            out.extend(collect_hoisted_vars(stmts).iter().map(|v| v.0));
            out.extend(crate::inline::collect_body_local_ids(stmts));
            for stmt in stmts {
                match stmt {
                    Stmt::If {
                        then_branch,
                        else_branch,
                        ..
                    } => {
                        ids(then_branch, out);
                        if let Some(b) = else_branch {
                            ids(b, out);
                        }
                    }
                    Stmt::While { body, .. }
                    | Stmt::DoWhile { body, .. }
                    | Stmt::For { body, .. } => ids(body, out),
                    Stmt::Try {
                        body,
                        catch,
                        finally,
                    } => {
                        ids(body, out);
                        if let Some(c) = catch {
                            ids(&c.body, out);
                        }
                        if let Some(b) = finally {
                            ids(b, out);
                        }
                    }
                    Stmt::Switch { cases, .. } => {
                        for c in cases {
                            ids(&c.body, out);
                        }
                    }
                    Stmt::Labeled { body, .. } => ids(std::slice::from_ref(body), out),
                    _ => {}
                }
            }
        }
        if let Expr::Closure { body, .. } = obj.as_ref() {
            ids(body, &mut locals);
        }
        let map: HashMap<_, _> = locals
            .into_iter()
            .map(|id| {
                let fresh = *next_local;
                *next_local += 1;
                (id, fresh)
            })
            .collect();
        let mut link = link;
        perry_hir::analysis::remap_local_ids_in_stmts(&mut setup, &map);
        perry_hir::analysis::remap_local_ids_in_expr(&mut link, &map);
        let Expr::LinkGeneratorPrototype {
            obj,
            i32_controls,
            i1_controls,
            ..
        } = &mut link
        else {
            unreachable!()
        };
        let Expr::Closure {
            func_id,
            body,
            captures,
            params,
            ..
        } = obj.as_mut()
        else {
            unreachable!()
        };
        for p in params.iter_mut() {
            if let Some(id) = map.get(&p.id) {
                p.id = *id;
            }
        }
        *func_id = *next_func;
        *next_func += 1;
        let resume_id = *func_id;
        fn catch_binders(stmts: &mut [Stmt], map: &HashMap<u32, u32>) {
            for s in stmts {
                if let Stmt::Let { id, .. } = s {
                    if let Some(fresh) = map.get(id) {
                        *id = *fresh;
                    }
                }
                if let Stmt::Try { catch: Some(c), .. } = s {
                    if let Some((id, _)) = &mut c.param {
                        if let Some(fresh) = map.get(id) {
                            *id = *fresh;
                        }
                    }
                }
                if let Stmt::For {
                    init: Some(init), ..
                } = s
                {
                    catch_binders(std::slice::from_mut(init.as_mut()), map);
                }
                if let Stmt::Labeled { body, .. } = s {
                    catch_binders(std::slice::from_mut(body.as_mut()), map);
                    continue;
                }
                for b in crate::closure_local_inline::nested_stmt_lists(s) {
                    catch_binders(b, map);
                }
            }
        }
        catch_binders(body, &map);
        catch_binders(&mut setup, &map);
        // The standalone runtime normally owns done/start/exception handling.
        // The private entry retains equivalent compiled boundaries.
        let done = captures[1];
        let executing = captures[3];
        let step = captures[4];
        let sent = captures[2];
        let arg = params.last().unwrap().id;
        if params.len() == 1 {
            let op = *next_local;
            *next_local += 1;
            params.insert(
                0,
                perry_hir::Param {
                    id: op,
                    name: "__frame_op".into(),
                    ty: Type::Number,
                    is_rest: false,
                    default: None,
                    decorators: vec![],
                    arguments_object: None,
                },
            );
            i32_controls.push(op);
            body.insert(
                0,
                Stmt::If {
                    condition: Expr::Compare {
                        op: CompareOp::Eq,
                        left: Box::new(Expr::LocalGet(op)),
                        right: Box::new(Expr::Number(1.0)),
                    },
                    then_branch: vec![
                        Stmt::Expr(Expr::LocalSet(done, Box::new(Expr::Bool(true)))),
                        Stmt::Expr(Expr::LocalSet(step, Box::new(Expr::Number(1.0)))),
                        Stmt::Return(Some(Expr::LocalGet(arg))),
                    ],
                    else_branch: None,
                },
            );
        }
        let mut prefix = vec![
            Stmt::If {
                condition: Expr::LocalGet(done),
                then_branch: vec![
                    Stmt::Expr(Expr::LocalSet(step, Box::new(Expr::Number(1.0)))),
                    Stmt::Return(Some(Expr::Undefined)),
                ],
                else_branch: None,
            },
            Stmt::Expr(Expr::LocalSet(sent, Box::new(Expr::LocalGet(arg)))),
            Stmt::Expr(Expr::LocalSet(executing, Box::new(Expr::Bool(true)))),
        ];
        prefix.append(body);
        let catch_id = *next_local;
        *next_local += 1;
        *body = super::lower::wrap_generator_resume_body(prefix, executing, done, catch_id, false);
        // The HIR remapper above also updates the explicit control identities.
        let _ = (i32_controls, i1_controls);
        let mut replacement = Vec::new();
        for (p, arg) in f.params.iter().zip(args) {
            replacement.push(Stmt::Let {
                id: map[&p.id],
                name: format!("__frame_arg{}", p.id),
                ty: Type::Any,
                mutable: false,
                init: Some(arg),
            });
        }
        replacement.extend(setup);
        replacement.push(Stmt::Expr(Expr::LocalSet(
            id,
            Box::new(runtime("privateResumeFrame", vec![link])),
        )));
        // Keep compiler-owned child slots lexical, including at module init:
        // they must never be promoted to mutable module globals.
        let mut declaration = stmts[at].clone();
        if let Stmt::Let { init, .. } = &mut declaration {
            *init = Some(Expr::Undefined);
        }
        replacement = vec![Stmt::If {
            condition: Expr::Bool(true),
            then_branch: replacement,
            else_branch: None,
        }];
        if !assigned {
            replacement.insert(0, declaration);
        }
        // Fresh cloned argument bindings cannot be referenced outside this
        // constructor. A private outer frame may retain the child's owner;
        // its traced capture is the only additional allowed use.
        let fresh_uses = reference_counts(&replacement);
        lower_list(
            &mut replacement,
            templates,
            &fresh_uses,
            next_local,
            next_func,
        );
        rewrite_uses(&mut stmts[at + 1..], id, resume_id);
        let count = replacement.len();
        stmts.splice(at..=at, replacement);
        at += count;
    }
}

fn rewrite_uses(stmts: &mut [Stmt], id: LocalId, fid: FuncId) {
    fn expr(e: &mut Expr, id: LocalId, fid: FuncId) {
        if let Expr::NativeMethodCall {
            module,
            method,
            args,
            ..
        } = e
        {
            if module == "__perry_runtime"
                && matches!(args.first(),Some(Expr::LocalGet(other)) if *other==id)
            {
                match method.as_str() {
                    "iteratorNextMethod" => {
                        *e = Expr::Undefined;
                        return;
                    }
                    "iteratorStep" => {
                        *e = runtime(
                            "privateResumeFrameStep",
                            vec![
                                args[0].clone(),
                                Expr::Integer(i64::from(fid)),
                                args[2].clone(),
                            ],
                        );
                        return;
                    }
                    "iteratorCloseIfNotDone" | "iteratorCloseOnThrow" => {
                        let on_throw = method == "iteratorCloseOnThrow";
                        let mut a = vec![args[0].clone(), Expr::Integer(i64::from(fid))];
                        a.extend(args.iter().skip(1).cloned());
                        *e = runtime(
                            if on_throw {
                                "privateResumeFrameCloseOnThrow"
                            } else {
                                "privateResumeFrameClose"
                            },
                            a,
                        );
                        return;
                    }
                    _ => {}
                }
            }
        }
        if let Expr::NativeMethodCall {
            module,
            method,
            args,
            ..
        } = e
        {
            if module == "__perry_runtime" && method == "privateResumeFrame" {
                if let Some(Expr::LinkGeneratorPrototype { obj, .. }) = args.first_mut() {
                    if let Expr::Closure { body, .. } = obj.as_mut() {
                        *body = super::private_frame_records::normalize(body, id);
                        rewrite_uses(body, id, fid);
                    }
                }
            }
        }
        perry_hir::walker::walk_expr_children_mut(e, &mut |e| expr(e, id, fid));
    }
    for s in stmts {
        crate::closure_local_inline::for_each_expr_in_stmt_mut(s, &mut |e| expr(e, id, fid));
        for b in crate::closure_local_inline::nested_stmt_lists(s) {
            rewrite_uses(b, id, fid);
        }
    }
}

#[cfg(test)]
#[path = "private_frame_tests.rs"]
mod tests;

pub(super) fn reference_counts(stmts: &[Stmt]) -> HashMap<u32, usize> {
    let mut uses = HashMap::new();
    visit_stmts(stmts, &mut |e| match e {
        Expr::LocalGet(id) | Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
            *uses.entry(*id).or_default() += 1;
        }
        Expr::Closure {
            captures,
            mutable_captures,
            ..
        } => {
            for id in captures.iter().chain(mutable_captures.iter()) {
                *uses.entry(*id).or_default() += 1;
            }
        }
        _ => {}
    });
    uses
}

/// A canonical resume/step closure is a traced owner over its entire suspended
/// lifetime. Remove exactly its own capture declaration from the reference
/// census, while retaining every other capture, alias and external use.
fn lower_owned_resume(
    f: &mut Function,
    templates: &HashMap<FuncId, Function>,
    uses: &HashMap<u32, usize>,
    next_local: &mut u32,
    next_func: &mut u32,
) {
    fn body(
        closure: &mut Expr,
        templates: &HashMap<FuncId, Function>,
        uses: &HashMap<u32, usize>,
        next_local: &mut u32,
        next_func: &mut u32,
    ) {
        if let Expr::Closure {
            body,
            captures,
            mutable_captures,
            ..
        } = closure
        {
            let mut owned = uses.clone();
            for id in captures.iter().chain(mutable_captures.iter()) {
                if let Some(count) = owned.get_mut(id) {
                    *count = count.saturating_sub(1);
                }
            }
            lower_list(body, templates, &owned, next_local, next_func);
        }
    }
    if f.was_plain_async {
        let step = match f.body.last() {
            Some(Stmt::Return(Some(Expr::AsyncFirstCall { step_closure }))) => {
                match step_closure.as_ref() {
                    Expr::LocalGet(id) => Some(*id),
                    _ => None,
                }
            }
            _ => None,
        };
        for s in &mut f.body {
            if let Stmt::Let {
                id,
                init: Some(value),
                ..
            } = s
            {
                if Some(*id) == step {
                    if let Expr::NativeMethodCall {
                        module,
                        method,
                        args,
                        ..
                    } = value
                    {
                        if module == "__perry_runtime" && method == "privateAsyncResumeFrame" {
                            if let Some(closure) = args.first_mut() {
                                body(closure, templates, uses, next_local, next_func);
                            }
                        }
                    } else {
                        body(value, templates, uses, next_local, next_func);
                    }
                }
            }
        }
    } else if let Some(Stmt::Return(Some(Expr::LinkGeneratorPrototype {
        obj,
        fixed_state: true,
        is_async: false,
        ..
    }))) = f.body.last_mut()
    {
        body(obj, templates, uses, next_local, next_func);
    }
}
