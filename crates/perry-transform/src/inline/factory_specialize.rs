use perry_hir::types::{FuncId, LocalId};
use perry_hir::walker::walk_expr_children;
use perry_hir::{Class, Expr, Module, Stmt};
use std::collections::{HashMap, HashSet};

use super::*;

#[derive(Clone, Debug)]
struct FactoryTarget {
    target_name: String,
    param_ids: Vec<LocalId>,
}

#[derive(Clone, Debug)]
struct CurriedFactoryTarget {
    target_name: String,
    outer_param_ids: Vec<LocalId>,
    inner_param_ids: Vec<LocalId>,
}

pub fn specialize_captured_class_factories(module: &mut Module) {
    // Build maps of class-returning factory functions. We specialize only
    // factories whose returned class reads factory params through synthesized
    // capture ctor params or a dynamic `extends` expression.
    let mut factory_targets: HashMap<FuncId, FactoryTarget> = HashMap::new();
    let mut curried_factory_targets: HashMap<FuncId, CurriedFactoryTarget> = HashMap::new();
    let class_index: HashMap<String, usize> = module
        .classes
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name.clone(), i))
        .collect();

    // Try to resolve the class name returned by `body`, given the
    // current module's classes. Recognizes:
    //   (a) `[Return(Some(ClassRef(C)))]` — single-stmt direct return.
    //   (b) `[Let { id: x, init: ClassRef(C) }, ..stmts.., Return(Some(LocalGet(x)))]`
    //       — anon class expression bound to a local, optional side effects,
    //       then returned.
    //   (c) `[Let { id: o, init: New { class_name: A, args } }, ..stmts..,
    //         Return(Some(PropertyGet { object: LocalGet(o), property: P }))]`
    //       — Effect-shape: object literal wrapping the class, optional
    //       side effects (prototype tweaks), then return `O.<P>`. The
    //       anon-shape class `A` has fields ordered to match `args`;
    //       resolve `P` to the field index, take `args[index]`, and
    //       require it to be a `ClassRef(C)`.
    //
    // For (b) and (c) the middle statements must not REASSIGN the bound
    // local (`x` / `o`). Statements that read it (e.g. `PropertySet` on
    // sub-properties for prototype mutation) are allowed.
    fn resolve_factory_return_class<'a>(body: &'a [Stmt], classes: &'a [Class]) -> Option<String> {
        if let [Stmt::Return(Some(expr))] = body {
            if let Some(c) = classref_name(expr) {
                return Some(c);
            }
        }
        if body.len() < 2 {
            return None;
        }
        // Look at the first and last stmts to detect shape (b) / (c).
        let last_idx = body.len() - 1;
        let Stmt::Return(Some(ret_expr)) = &body[last_idx] else {
            return None;
        };
        let Stmt::Let {
            id: bound_id,
            init: Some(init_expr),
            ..
        } = &body[0]
        else {
            return None;
        };
        // Middle stmts (between the Let and the Return) must not reassign
        // the bound local. We do a conservative check via `LocalSet(bound_id, _)`
        // or `Update { id: bound_id, .. }` shapes.
        for middle in &body[1..last_idx] {
            if !middle_stmt_is_safe(middle, *bound_id) {
                return None;
            }
        }
        match (init_expr, ret_expr) {
            (init, Expr::LocalGet(x_ref)) if *x_ref == *bound_id => classref_name(init),
            (
                Expr::New {
                    class_name: anon_name,
                    args,
                    ..
                },
                Expr::PropertyGet {
                    object, property, ..
                },
            ) => {
                if let Expr::LocalGet(o_ref) = object.as_ref() {
                    if *o_ref != *bound_id {
                        return None;
                    }
                } else {
                    return None;
                }
                let anon = classes.iter().find(|c| c.name == *anon_name)?;
                let field_idx = anon.fields.iter().position(|f| f.name == *property)?;
                let arg = args.get(field_idx)?;
                if let Expr::ClassRef(c) = arg {
                    Some(c.clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn classref_name(expr: &Expr) -> Option<String> {
        match expr {
            Expr::ClassRef(c) => Some(c.clone()),
            Expr::Sequence(parts) => parts.last().and_then(classref_name),
            _ => None,
        }
    }

    fn resolve_curried_factory_return_class<'a>(
        body: &'a [Stmt],
        classes: &'a [Class],
    ) -> Option<(String, Vec<LocalId>)> {
        let [Stmt::Return(Some(Expr::Closure { params, body, .. }))] = body else {
            return None;
        };
        let target = resolve_factory_return_class(body, classes)?;
        Some((target, params.iter().map(|p| p.id).collect()))
    }

    fn middle_stmt_is_safe(stmt: &Stmt, bound_id: LocalId) -> bool {
        // Conservative: allow Stmt::Expr where the expression doesn't
        // mutate `bound_id` directly. Other stmt kinds (Let, If, While,
        // For, …) are rare in a factory function between the bound-Let
        // and the Return — bail out if we see one. (The factory pattern
        // tracked here is short and linear; deeper shapes can be
        // supported in a follow-up.)
        match stmt {
            Stmt::Expr(e) => !expr_writes_local(e, bound_id),
            _ => false,
        }
    }

    fn expr_writes_local(expr: &Expr, bound_id: LocalId) -> bool {
        match expr {
            Expr::LocalSet(id, _) | Expr::Update { id, .. } => *id == bound_id,
            _ => {
                let mut hit = false;
                walk_expr_children(expr, &mut |child| {
                    if expr_writes_local(child, bound_id) {
                        hit = true;
                    }
                });
                hit
            }
        }
    }

    fn class_needs_specialization(class: &Class, param_ids: &[LocalId]) -> bool {
        let param_set: HashSet<LocalId> = param_ids.iter().copied().collect();
        let has_capture_params = class
            .constructor
            .as_ref()
            .map(|c| {
                c.params.iter().any(|p| {
                    perry_hir::cap_fields::cap_field_outer_id(&p.name)
                        .map(|outer_id| param_set.contains(&outer_id))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        if has_capture_params {
            return true;
        }
        class
            .extends_expr
            .as_deref()
            .map(|expr| expr_reads_any_local(expr, &param_set))
            .unwrap_or(false)
    }

    fn expr_reads_any_local(expr: &Expr, ids: &HashSet<LocalId>) -> bool {
        if let Expr::LocalGet(id) = expr {
            return ids.contains(id);
        }
        let mut found = false;
        walk_expr_children(expr, &mut |child| {
            if !found && expr_reads_any_local(child, ids) {
                found = true;
            }
        });
        found
    }

    for f in &module.functions {
        let param_ids: Vec<LocalId> = f.params.iter().map(|p| p.id).collect();

        if let Some(target) = resolve_factory_return_class(&f.body, &module.classes) {
            if let Some(&ci) = class_index.get(&target) {
                let class = &module.classes[ci];
                if class_needs_specialization(class, &param_ids) {
                    factory_targets.insert(
                        f.id,
                        FactoryTarget {
                            target_name: target,
                            param_ids: param_ids.clone(),
                        },
                    );
                }
            }
        }

        if let Some((target, inner_param_ids)) =
            resolve_curried_factory_return_class(&f.body, &module.classes)
        {
            if let Some(&ci) = class_index.get(&target) {
                let class = &module.classes[ci];
                let mut combined_param_ids = param_ids.clone();
                combined_param_ids.extend(inner_param_ids.iter().copied());
                if class_needs_specialization(class, &combined_param_ids) {
                    curried_factory_targets.insert(
                        f.id,
                        CurriedFactoryTarget {
                            target_name: target,
                            outer_param_ids: param_ids,
                            inner_param_ids,
                        },
                    );
                }
            }
        }
    }
    if factory_targets.is_empty() && curried_factory_targets.is_empty() {
        return;
    }

    // Walk all places that can host a `Let { init: Call(...) }`: module.init,
    // function bodies, class ctor bodies, method bodies, getter/setter
    // bodies. Each gets a separate visit. Per-call class clones append to
    // `new_classes` and flush at the end. `next_class_counter` makes the
    // synthesized name unique within this module.
    let mut new_classes: Vec<Class> = Vec::new();
    let mut next_class_counter: usize = 0;

    // Helper: visit a slice of stmts and rewrite eligible Lets in place.
    fn visit_stmts(
        stmts: &mut [Stmt],
        factory_targets: &HashMap<FuncId, FactoryTarget>,
        curried_factory_targets: &HashMap<FuncId, CurriedFactoryTarget>,
        classes: &[Class],
        new_classes: &mut Vec<Class>,
        next_class_counter: &mut usize,
        base_class_counter_seed: &str,
    ) {
        for stmt in stmts.iter_mut() {
            match stmt {
                Stmt::Let { init: Some(e), .. } => {
                    rewrite_call_init(
                        e,
                        factory_targets,
                        curried_factory_targets,
                        classes,
                        new_classes,
                        next_class_counter,
                        base_class_counter_seed,
                    );
                }
                Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => {
                    rewrite_call_init(
                        e,
                        factory_targets,
                        curried_factory_targets,
                        classes,
                        new_classes,
                        next_class_counter,
                        base_class_counter_seed,
                    );
                }
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    rewrite_call_init(
                        condition,
                        factory_targets,
                        curried_factory_targets,
                        classes,
                        new_classes,
                        next_class_counter,
                        base_class_counter_seed,
                    );
                    visit_stmts(
                        then_branch,
                        factory_targets,
                        curried_factory_targets,
                        classes,
                        new_classes,
                        next_class_counter,
                        base_class_counter_seed,
                    );
                    if let Some(eb) = else_branch {
                        visit_stmts(
                            eb,
                            factory_targets,
                            curried_factory_targets,
                            classes,
                            new_classes,
                            next_class_counter,
                            base_class_counter_seed,
                        );
                    }
                }
                Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                    rewrite_call_init(
                        condition,
                        factory_targets,
                        curried_factory_targets,
                        classes,
                        new_classes,
                        next_class_counter,
                        base_class_counter_seed,
                    );
                    visit_stmts(
                        body,
                        factory_targets,
                        curried_factory_targets,
                        classes,
                        new_classes,
                        next_class_counter,
                        base_class_counter_seed,
                    );
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    if let Some(init_stmt) = init {
                        let mut tmp = vec![*init_stmt.clone()];
                        visit_stmts(
                            &mut tmp,
                            factory_targets,
                            curried_factory_targets,
                            classes,
                            new_classes,
                            next_class_counter,
                            base_class_counter_seed,
                        );
                        if tmp.len() == 1 {
                            **init_stmt = tmp.remove(0);
                        }
                    }
                    if let Some(c) = condition {
                        rewrite_call_init(
                            c,
                            factory_targets,
                            curried_factory_targets,
                            classes,
                            new_classes,
                            next_class_counter,
                            base_class_counter_seed,
                        );
                    }
                    if let Some(u) = update {
                        rewrite_call_init(
                            u,
                            factory_targets,
                            curried_factory_targets,
                            classes,
                            new_classes,
                            next_class_counter,
                            base_class_counter_seed,
                        );
                    }
                    visit_stmts(
                        body,
                        factory_targets,
                        curried_factory_targets,
                        classes,
                        new_classes,
                        next_class_counter,
                        base_class_counter_seed,
                    );
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    visit_stmts(
                        body,
                        factory_targets,
                        curried_factory_targets,
                        classes,
                        new_classes,
                        next_class_counter,
                        base_class_counter_seed,
                    );
                    if let Some(c) = catch {
                        visit_stmts(
                            &mut c.body,
                            factory_targets,
                            curried_factory_targets,
                            classes,
                            new_classes,
                            next_class_counter,
                            base_class_counter_seed,
                        );
                    }
                    if let Some(fin) = finally {
                        visit_stmts(
                            fin,
                            factory_targets,
                            curried_factory_targets,
                            classes,
                            new_classes,
                            next_class_counter,
                            base_class_counter_seed,
                        );
                    }
                }
                Stmt::Switch {
                    discriminant,
                    cases,
                } => {
                    rewrite_call_init(
                        discriminant,
                        factory_targets,
                        curried_factory_targets,
                        classes,
                        new_classes,
                        next_class_counter,
                        base_class_counter_seed,
                    );
                    for case in cases {
                        if let Some(t) = &mut case.test {
                            rewrite_call_init(
                                t,
                                factory_targets,
                                curried_factory_targets,
                                classes,
                                new_classes,
                                next_class_counter,
                                base_class_counter_seed,
                            );
                        }
                        visit_stmts(
                            &mut case.body,
                            factory_targets,
                            curried_factory_targets,
                            classes,
                            new_classes,
                            next_class_counter,
                            base_class_counter_seed,
                        );
                    }
                }
                Stmt::Labeled { body, .. } => {
                    let mut tmp = vec![*body.clone()];
                    visit_stmts(
                        &mut tmp,
                        factory_targets,
                        curried_factory_targets,
                        classes,
                        new_classes,
                        next_class_counter,
                        base_class_counter_seed,
                    );
                    if tmp.len() == 1 {
                        **body = tmp.remove(0);
                    }
                }
                _ => {}
            }
        }
    }

    // Helper: if `expr` is a Call to a factory function, rewrite it to a
    // ClassRef of a freshly-specialized clone of the target class. Also
    // recurses into sub-expressions so nested factory calls inside e.g.
    // an Array literal still get specialized.
    fn rewrite_call_init(
        expr: &mut Expr,
        factory_targets: &HashMap<FuncId, FactoryTarget>,
        curried_factory_targets: &HashMap<FuncId, CurriedFactoryTarget>,
        classes: &[Class],
        new_classes: &mut Vec<Class>,
        next_class_counter: &mut usize,
        base_class_counter_seed: &str,
    ) {
        // First, recurse so deeply-nested calls are rewritten bottom-up. We
        // use a manual walker so the recursion only descends into bits we
        // care about (Call args, conditional branches, ...). This is also
        // important because the post-rewrite expression may itself contain
        // a Call we don't want to recurse into a second time.
        match expr {
            Expr::Call { callee, args, .. } => {
                rewrite_call_init(
                    callee,
                    factory_targets,
                    curried_factory_targets,
                    classes,
                    new_classes,
                    next_class_counter,
                    base_class_counter_seed,
                );
                for a in args.iter_mut() {
                    rewrite_call_init(
                        a,
                        factory_targets,
                        curried_factory_targets,
                        classes,
                        new_classes,
                        next_class_counter,
                        base_class_counter_seed,
                    );
                }
            }
            Expr::Binary { left, right, .. }
            | Expr::Logical { left, right, .. }
            | Expr::Compare { left, right, .. } => {
                rewrite_call_init(
                    left,
                    factory_targets,
                    curried_factory_targets,
                    classes,
                    new_classes,
                    next_class_counter,
                    base_class_counter_seed,
                );
                rewrite_call_init(
                    right,
                    factory_targets,
                    curried_factory_targets,
                    classes,
                    new_classes,
                    next_class_counter,
                    base_class_counter_seed,
                );
            }
            Expr::Unary { operand, .. } => {
                rewrite_call_init(
                    operand,
                    factory_targets,
                    curried_factory_targets,
                    classes,
                    new_classes,
                    next_class_counter,
                    base_class_counter_seed,
                );
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                rewrite_call_init(
                    condition,
                    factory_targets,
                    curried_factory_targets,
                    classes,
                    new_classes,
                    next_class_counter,
                    base_class_counter_seed,
                );
                rewrite_call_init(
                    then_expr,
                    factory_targets,
                    curried_factory_targets,
                    classes,
                    new_classes,
                    next_class_counter,
                    base_class_counter_seed,
                );
                rewrite_call_init(
                    else_expr,
                    factory_targets,
                    curried_factory_targets,
                    classes,
                    new_classes,
                    next_class_counter,
                    base_class_counter_seed,
                );
            }
            Expr::Array(elems) => {
                for e in elems.iter_mut() {
                    rewrite_call_init(
                        e,
                        factory_targets,
                        curried_factory_targets,
                        classes,
                        new_classes,
                        next_class_counter,
                        base_class_counter_seed,
                    );
                }
            }
            Expr::RegisterClassParentDynamic { parent_expr, .. } => {
                rewrite_call_init(
                    parent_expr,
                    factory_targets,
                    curried_factory_targets,
                    classes,
                    new_classes,
                    next_class_counter,
                    base_class_counter_seed,
                );
            }
            Expr::RegisterClassStaticSymbol {
                key_expr,
                value_expr,
                ..
            } => {
                rewrite_call_init(
                    key_expr,
                    factory_targets,
                    curried_factory_targets,
                    classes,
                    new_classes,
                    next_class_counter,
                    base_class_counter_seed,
                );
                rewrite_call_init(
                    value_expr,
                    factory_targets,
                    curried_factory_targets,
                    classes,
                    new_classes,
                    next_class_counter,
                    base_class_counter_seed,
                );
            }
            Expr::New { args, .. } => {
                for a in args.iter_mut() {
                    rewrite_call_init(
                        a,
                        factory_targets,
                        curried_factory_targets,
                        classes,
                        new_classes,
                        next_class_counter,
                        base_class_counter_seed,
                    );
                }
            }
            Expr::PropertyGet { object, .. } => {
                rewrite_call_init(
                    object,
                    factory_targets,
                    curried_factory_targets,
                    classes,
                    new_classes,
                    next_class_counter,
                    base_class_counter_seed,
                );
            }
            Expr::PropertySet { object, value, .. } => {
                rewrite_call_init(
                    object,
                    factory_targets,
                    curried_factory_targets,
                    classes,
                    new_classes,
                    next_class_counter,
                    base_class_counter_seed,
                );
                rewrite_call_init(
                    value,
                    factory_targets,
                    curried_factory_targets,
                    classes,
                    new_classes,
                    next_class_counter,
                    base_class_counter_seed,
                );
            }
            _ => {}
        }
        // Now detect the factory pattern at THIS level.
        let curried_call = match expr {
            Expr::Call { callee, args, .. } => {
                if let Expr::Call {
                    callee: outer_callee,
                    args: outer_args,
                    ..
                } = callee.as_ref()
                {
                    if let Expr::FuncRef(fn_id) = outer_callee.as_ref() {
                        Some((*fn_id, outer_args.clone(), args.clone()))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => return,
        };
        if let Some((fn_id, outer_args, inner_args)) = curried_call {
            if let Some(target) = curried_factory_targets.get(&fn_id) {
                let mut param_ids = target.outer_param_ids.clone();
                param_ids.extend(target.inner_param_ids.iter().copied());
                let mut call_args = outer_args;
                call_args.extend(inner_args);
                if rewrite_to_specialized_class(
                    expr,
                    &target.target_name,
                    &param_ids,
                    &call_args,
                    classes,
                    new_classes,
                    next_class_counter,
                    base_class_counter_seed,
                ) {
                    return;
                }
            }
        }

        let direct_call = match expr {
            Expr::Call { callee, args, .. } => {
                if let Expr::FuncRef(fn_id) = callee.as_ref() {
                    Some((*fn_id, args.clone()))
                } else {
                    None
                }
            }
            _ => None,
        };
        let Some((fn_id, call_args)) = direct_call else {
            return;
        };
        let Some(target) = factory_targets.get(&fn_id) else {
            return;
        };
        rewrite_to_specialized_class(
            expr,
            &target.target_name,
            &target.param_ids,
            &call_args,
            classes,
            new_classes,
            next_class_counter,
            base_class_counter_seed,
        );
    }

    fn rewrite_to_specialized_class(
        expr: &mut Expr,
        target_name: &str,
        param_ids: &[LocalId],
        args: &[Expr],
        classes: &[Class],
        new_classes: &mut Vec<Class>,
        next_class_counter: &mut usize,
        base_class_counter_seed: &str,
    ) -> bool {
        let Some(class) = classes.iter().find(|c| c.name == target_name) else {
            return false;
        };
        // Snapshot the args, padding with Undefined if the call passes
        // fewer args than the function declared params (rare but legal).
        let mut padded_args: Vec<Expr> = args.to_vec();
        while padded_args.len() < param_ids.len() {
            padded_args.push(Expr::Undefined);
        }
        // Build substitution map: ctor-param-id (of __perry_cap_<outer_id>)
        // → corresponding call arg expression. The mapping is keyed by the
        // SYNTHESIZED ctor param's id (the value referenced by LocalGet
        // inside method/field-init bodies), not the outer_id encoded in the
        // name. We translate via the param NAME (`__perry_cap_<outer_id>`)
        // → the index of `outer_id` in `param_ids` → `padded_args[index]`.
        let mut subst: HashMap<LocalId, Expr> = HashMap::new();
        let param_subst: HashMap<LocalId, Expr> = param_ids
            .iter()
            .enumerate()
            .map(|(idx, id)| {
                (
                    *id,
                    padded_args.get(idx).cloned().unwrap_or(Expr::Undefined),
                )
            })
            .collect();
        if let Some(ctor) = &class.constructor {
            for p in &ctor.params {
                if let Some(outer_id) = perry_hir::cap_fields::cap_field_outer_id(&p.name) {
                    {
                        if let Some(idx) = param_ids.iter().position(|id| *id == outer_id) {
                            let arg_expr = padded_args[idx].clone();
                            subst.insert(p.id, arg_expr);
                        } else {
                            // Capture isn't a param of `f` (might be an
                            // outer-of-outer capture chained from an
                            // enclosing function). For the #740 fix scope
                            // we only handle direct captures of the
                            // factory's own params; bail out of
                            // specialization in this case and leave the
                            // Call as-is so any later pass can handle it.
                            return false;
                        }
                    }
                }
            }
        }
        let param_set: HashSet<LocalId> = param_ids.iter().copied().collect();
        let extends_needs_subst = class
            .extends_expr
            .as_deref()
            .map(|expr| expr_reads_any_local(expr, &param_set))
            .unwrap_or(false);
        if subst.is_empty() && !extends_needs_subst {
            return false;
        }
        // Clone and specialize the class.
        let mut next_id_seed: LocalId = 0;
        let cloned_name = format!(
            "{}__inline_{}_{}",
            target_name, base_class_counter_seed, *next_class_counter
        );
        *next_class_counter += 1;
        let mut cloned = class.clone();
        cloned.name = cloned_name.clone();
        // When the parent arg specializes to a runtime value rather than a
        // static class (`Serializable(Mid)` where `Mid` is a local binding, not
        // a `ClassRef`), we can't wire a static `extends_name`. The factory
        // Call — including the `RegisterClassParentDynamic` its lowered Sequence
        // carried — is about to be replaced by a bare `ClassRef` below, which
        // would drop the parent edge entirely: `new Top()` would then inherit
        // nothing from `Mid` (no fields, no methods, `instanceof Mid` false).
        // Re-emit the dynamic registration at the call site instead (#6356).
        let mut dynamic_parent_expr: Option<Expr> = None;
        if let Some(extends_expr) = cloned.extends_expr.as_mut() {
            substitute_locals(extends_expr, &param_subst, &mut next_id_seed);
            if let Expr::ClassRef(parent_name) = extends_expr.as_ref() {
                cloned.extends_name = Some(parent_name.clone());
                cloned.extends = classes
                    .iter()
                    .find(|c| &c.name == parent_name)
                    .map(|c| c.id)
                    .or_else(|| {
                        new_classes
                            .iter()
                            .find(|c| &c.name == parent_name)
                            .map(|c| c.id)
                    });
            } else {
                dynamic_parent_expr = Some((**extends_expr).clone());
            }
        }
        // Filter out the capture ctor params + matching synthetic fields +
        // ctor-body assignments. Substitute the captured-param LocalGets
        // with the bound arg expression throughout the class body.
        if !subst.is_empty() {
            if let Some(ctor) = cloned.constructor.as_mut() {
                // Identify the synthetic ctor param ids and the names we need
                // to drop from fields and ctor body.
                let cap_param_ids: HashSet<LocalId> = ctor
                    .params
                    .iter()
                    .filter(|p| p.name.starts_with("__perry_cap_"))
                    .map(|p| p.id)
                    .collect();
                let cap_field_names: HashSet<String> = ctor
                    .params
                    .iter()
                    .filter(|p| p.name.starts_with("__perry_cap_"))
                    .map(|p| p.name.clone())
                    .collect();
                // Drop the capture ctor params.
                ctor.params.retain(|p| !cap_param_ids.contains(&p.id));
                // Substitute remaining body refs to those param ids.
                substitute_locals_in_stmts(&mut ctor.body, &subst, &mut next_id_seed);
                // Drop ctor body statements that were the synthesized
                // assignment `this.__perry_cap_<outer> = LocalGet(...)`. After
                // substitution above those LocalGets are gone, so the assign
                // would write a useless value to a field we're about to
                // remove — drop them to keep the ctor body minimal.
                ctor.body.retain(|s| match s {
                    Stmt::Expr(Expr::PropertySet { property, .. }) => {
                        !cap_field_names.contains(property)
                    }
                    _ => true,
                });
                // Drop the synthetic capture fields.
                cloned.fields.retain(|f| !cap_field_names.contains(&f.name));
                // Substitute remaining field inits / key exprs.
                for field in cloned.fields.iter_mut() {
                    if let Some(init) = field.init.as_mut() {
                        substitute_locals(init, &subst, &mut next_id_seed);
                    }
                    if let Some(key) = field.key_expr.as_mut() {
                        substitute_locals(key, &subst, &mut next_id_seed);
                    }
                }
                // Substitute in methods/getters/setters.
                for m in cloned.methods.iter_mut() {
                    substitute_locals_in_stmts(&mut m.body, &subst, &mut next_id_seed);
                }
                for (_, g) in cloned.getters.iter_mut() {
                    substitute_locals_in_stmts(&mut g.body, &subst, &mut next_id_seed);
                }
                for (_, s) in cloned.setters.iter_mut() {
                    substitute_locals_in_stmts(&mut s.body, &subst, &mut next_id_seed);
                }
            }
        }
        // Avoid `aliases` pointing at the original class — the clone is a
        // standalone class with its own identity.
        cloned.aliases.clear();
        cloned.is_exported = false;
        // Issue #740: if the only thing the synthesized ctor used to do was
        // assign captures (now baked in as constants in fields) — i.e. the
        // ctor has no params and an empty body — drop the ctor entirely.
        // Otherwise codegen's `lower_new` finds the empty ctor and STOPS
        // its parent-walk there, which prevents the real ancestor's ctor
        // (e.g. `BaseError(opts)` setting `this.issue = opts.issue`) from
        // running when a child like `ParseError` is constructed. With no
        // ctor at all the parent walk continues up to the first user-
        // written ancestor ctor.
        if let Some(ctor) = cloned.constructor.as_ref() {
            if ctor.params.is_empty() && ctor.body.is_empty() {
                cloned.constructor = None;
            }
        }
        // Assign a FRESH, unique class id. `class.clone()` above copied the
        // template's id, and codegen keys the runtime class registry (methods,
        // constructor, parent edges) by `c.id` (`codegen/mod.rs` builds
        // `class_ids` from each class's `id`). Leaving the copy in place makes a
        // clone collide with its template AND with sibling clones of the same
        // template (`Serializable(Greetable(Base))` and `Serializable(Mid)` both
        // clone `__anon_class_N`) — the distinct classes then share one registry
        // slot, last-writer-wins on the constructor/parent edge, and a two-level
        // dynamic mixin chain loses its intermediate parent (#6356). Ids past
        // the current max are free: codegen derives its imported-class fallback
        // range from the post-specialization max, and static parent edges are
        // resolved by NAME, so only the numeric id has to be unique here. Later
        // clones referencing this one as a static parent (see the `extends`
        // lookup above) read the id assigned here, so assign before pushing.
        let base_class_id = classes.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        cloned.id = base_class_id + new_classes.len() as u32;
        new_classes.push(cloned);
        // Replace the Call with `ClassRef(cloned_name)`. The Let's init is
        // now a plain ClassRef — the regular inliner won't touch it and
        // subsequent `new <X>()` sites will see it as an alias for the
        // specialized class via the existing `local_class_aliases`
        // mechanism in codegen. When the parent is a runtime value, sequence
        // the dynamic parent registration ahead of the ClassRef (the Sequence
        // still yields the ClassRef as its value) so the parent edge is wired
        // when this init runs — mirroring the class-expression lowering in
        // `perry-hir`'s `arm_class.rs`.
        *expr = match dynamic_parent_expr {
            Some(parent_expr) => Expr::Sequence(vec![
                Expr::RegisterClassParentDynamic {
                    class_name: cloned_name.clone(),
                    parent_expr: Box::new(parent_expr),
                },
                Expr::ClassRef(cloned_name),
            ]),
            None => Expr::ClassRef(cloned_name),
        };
        true
    }

    // Visit module init.
    let classes_snapshot = module.classes.clone();
    visit_stmts(
        &mut module.init,
        &factory_targets,
        &curried_factory_targets,
        &classes_snapshot,
        &mut new_classes,
        &mut next_class_counter,
        "init",
    );
    // Dynamic parent registration with a now-static class result: after
    // `rewrite_call_init` replaces `RegisterClassParentDynamic {
    // parent_expr: Call(...) }` with `parent_expr: ClassRef(<inline>)`,
    // hoist that concrete parent into `class.extends_name` on the child. This
    // lets `lower_new` walk through the specialized class for inherited
    // constructors and field initializers instead of the unspecialized
    // anonymous original.
    //
    // `parent_expr` may be an `Expr::Sequence([RegisterClassParentDynamic, …,
    // ClassRef(<inline>)])` when a class DECLARATION extends a factory call
    // with a runtime-value arg (`class Child extends Factory(runtimeVar) {}`):
    // `rewrite_call_init` rewrote the inner factory Call to that Sequence
    // (see `rewrite_to_specialized_class`). Resolve through `classref_name`,
    // which unwraps a Sequence to its trailing `ClassRef`, so the child's
    // static parent still hoists rather than falling back to the runtime edge.
    for stmt in &module.init {
        if let Stmt::Expr(Expr::RegisterClassParentDynamic {
            class_name,
            parent_expr,
        }) = stmt
        {
            if let Some(ref parent_name) = classref_name(parent_expr.as_ref()) {
                if let Some(child) = module.classes.iter_mut().find(|c| &c.name == class_name) {
                    child.extends_name = Some(parent_name.clone());
                    if let Some(parent_cls) = new_classes
                        .iter()
                        .find(|c| &c.name == parent_name)
                        .map(|c| c.id)
                        .or_else(|| {
                            classes_snapshot
                                .iter()
                                .find(|c| &c.name == parent_name)
                                .map(|c| c.id)
                        })
                    {
                        child.extends = Some(parent_cls);
                    }
                }
            }
        }
    }
    // Visit function bodies.
    for (fi, func) in module.functions.iter_mut().enumerate() {
        visit_stmts(
            &mut func.body,
            &factory_targets,
            &curried_factory_targets,
            &classes_snapshot,
            &mut new_classes,
            &mut next_class_counter,
            &format!("fn{}", fi),
        );
    }
    // Visit class ctor / method / getter / setter bodies.
    for (ci, class) in module.classes.iter_mut().enumerate() {
        if let Some(ctor) = class.constructor.as_mut() {
            visit_stmts(
                &mut ctor.body,
                &factory_targets,
                &curried_factory_targets,
                &classes_snapshot,
                &mut new_classes,
                &mut next_class_counter,
                &format!("c{}ctor", ci),
            );
        }
        for (mi, m) in class.methods.iter_mut().enumerate() {
            visit_stmts(
                &mut m.body,
                &factory_targets,
                &curried_factory_targets,
                &classes_snapshot,
                &mut new_classes,
                &mut next_class_counter,
                &format!("c{}m{}", ci, mi),
            );
        }
        for (gi, (_, g)) in class.getters.iter_mut().enumerate() {
            visit_stmts(
                &mut g.body,
                &factory_targets,
                &curried_factory_targets,
                &classes_snapshot,
                &mut new_classes,
                &mut next_class_counter,
                &format!("c{}g{}", ci, gi),
            );
        }
        for (si, (_, s)) in class.setters.iter_mut().enumerate() {
            visit_stmts(
                &mut s.body,
                &factory_targets,
                &curried_factory_targets,
                &classes_snapshot,
                &mut new_classes,
                &mut next_class_counter,
                &format!("c{}s{}", ci, si),
            );
        }
    }
    // Flush new specialized classes.
    module.classes.extend(new_classes);
}
