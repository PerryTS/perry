//! Constructive return proofs for guarded ordinary-parameter clones.
//!
//! A declared return annotation is not evidence. This pass issues a fact only
//! when every returned value can be derived from the clone's guarded inputs,
//! literal/runtime constructors, or another function carrying the same fact.
//! The fixed-point removal makes recursive groups possible without allowing an
//! unverified function into the final set.

use std::collections::{HashMap, HashSet};

use perry_hir::types::{ObjectType, PropertyInfo, Type};
use perry_hir::{Class, Expr, Function, Module, Stmt};

use super::spec_abi::{spec_ta_kind_class_name, SpecFnPlan};
use crate::collectors::SpecParamRep;

struct ProofCtx<'a> {
    functions: HashMap<u32, &'a Function>,
    classes: HashMap<String, &'a Class>,
    plans: &'a HashMap<u32, SpecFnPlan>,
    aliases: &'a HashMap<String, Type>,
    candidates: &'a HashSet<u32>,
}

fn normalize(aliases: &HashMap<String, Type>, ty: &Type) -> Type {
    let mut current = ty.clone();
    for _ in 0..32 {
        let Type::Named(name) = &current else {
            break;
        };
        let Some(next) = aliases.get(name) else {
            break;
        };
        if next == &current {
            break;
        }
        current = next.clone();
    }
    current
}

fn assignable(
    aliases: &HashMap<String, Type>,
    actual: &Type,
    expected: &Type,
    depth: usize,
) -> bool {
    if actual == expected {
        return true;
    }
    if depth > 32 {
        return false;
    }
    let actual = normalize(aliases, actual);
    let expected = normalize(aliases, expected);
    if actual == expected {
        return true;
    }
    match (&actual, &expected) {
        // `never` is the element type of a constructively empty array and is
        // the usual bottom type: no runtime value can violate `expected`.
        (Type::Never, _) => true,
        (Type::Int32, Type::Number) | (Type::StringLiteral(_), Type::String) => true,
        (Type::Union(actual), _) => actual
            .iter()
            .all(|variant| assignable(aliases, variant, &expected, depth + 1)),
        (_, Type::Union(expected)) => expected
            .iter()
            .any(|variant| assignable(aliases, &actual, variant, depth + 1)),
        (Type::Array(actual), Type::Array(expected)) => {
            assignable(aliases, actual, expected, depth + 1)
        }
        (Type::Tuple(actual), Type::Tuple(expected)) if actual.len() == expected.len() => actual
            .iter()
            .zip(expected)
            .all(|(a, e)| assignable(aliases, a, e, depth + 1)),
        (Type::Object(actual), Type::Object(expected)) => {
            expected.properties.iter().all(|(name, expected_property)| {
                if expected_property.optional {
                    return false;
                }
                actual.properties.get(name).is_some_and(|actual_property| {
                    !actual_property.optional
                        && assignable(
                            aliases,
                            &actual_property.ty,
                            &expected_property.ty,
                            depth + 1,
                        )
                })
            })
        }
        _ => false,
    }
}

fn property_type(ctx: &ProofCtx<'_>, owner: &Type, property: &str, depth: usize) -> Option<Type> {
    if depth > 32 {
        return None;
    }
    match owner {
        Type::Named(name) => property_type(ctx, ctx.aliases.get(name)?, property, depth + 1),
        Type::Object(object) => object
            .properties
            .get(property)
            .and_then(|field| (!field.optional).then(|| field.ty.clone())),
        Type::Union(variants) => {
            let mut found = Vec::new();
            for variant in variants {
                if let Some(ty) = property_type(ctx, variant, property, depth + 1) {
                    if !found.contains(&ty) {
                        found.push(ty);
                    }
                }
            }
            match found.len() {
                0 => None,
                1 => found.pop(),
                _ => Some(Type::Union(found)),
            }
        }
        _ => None,
    }
}

fn plan_param_proofs(function: &Function, plan: &SpecFnPlan) -> HashMap<u32, Type> {
    function
        .params
        .iter()
        .zip(plan.reps.iter())
        .zip(plan.guards.iter())
        .filter_map(|((param, rep), guard)| {
            let proof = match (guard, rep) {
                (Some(guard), _) => guard.proof.clone(),
                (None, SpecParamRep::I32) => Type::Int32,
                (None, SpecParamRep::F64) => Type::Number,
                (None, SpecParamRep::TaPtr { kind, .. }) => {
                    Type::Named(spec_ta_kind_class_name(*kind)?.to_string())
                }
                (None, SpecParamRep::Boxed) => return None,
            };
            Some((param.id, proof))
        })
        .collect()
}

fn call_is_proven(
    ctx: &ProofCtx<'_>,
    locals: &HashMap<u32, Type>,
    function_id: u32,
    args: &[Expr],
) -> bool {
    let (Some(function), Some(plan)) = (
        ctx.functions.get(&function_id).copied(),
        ctx.plans.get(&function_id),
    ) else {
        return false;
    };
    if !ctx.candidates.contains(&function_id)
        || function.params.len() != args.len()
        || plan.reps.len() != args.len()
    {
        return false;
    }
    function
        .params
        .iter()
        .zip(plan.reps.iter())
        .zip(plan.guards.iter())
        .zip(args.iter())
        .all(|(((_param, rep), guard), arg)| {
            let expected = match (guard, rep) {
                (Some(guard), _) => &guard.proof,
                (None, SpecParamRep::I32) => &Type::Int32,
                (None, SpecParamRep::F64) => &Type::Number,
                // The verifier currently handles ordinary boxed/scalar plans.
                // TaPtr's construction proof stays in its existing call-site
                // machinery and cannot publish a return fact here.
                (None, SpecParamRep::TaPtr { .. }) | (None, SpecParamRep::Boxed) => return false,
            };
            expr_proves(ctx, locals, arg, expected, 0)
        })
}

fn infer_expr(
    ctx: &ProofCtx<'_>,
    locals: &HashMap<u32, Type>,
    expr: &Expr,
    depth: usize,
) -> Option<Type> {
    if depth > 64 {
        return None;
    }
    match expr {
        Expr::Undefined | Expr::Void(_) => Some(Type::Void),
        Expr::Null => Some(Type::Null),
        Expr::Bool(_) | Expr::Compare { .. } => Some(Type::Boolean),
        Expr::Integer(value) if i32::try_from(*value).is_ok() => Some(Type::Int32),
        Expr::Integer(_) | Expr::Number(_) => Some(Type::Number),
        Expr::String(value) => Some(Type::StringLiteral(value.clone())),
        Expr::WtfString(_) | Expr::I18nString { .. } | Expr::TypeOf(_) => Some(Type::String),
        Expr::BigInt(_) => Some(Type::BigInt),
        Expr::LocalGet(id) => locals.get(id).cloned(),
        Expr::PropertyGet {
            object, property, ..
        } => property_type(
            ctx,
            &infer_expr(ctx, locals, object, depth + 1)?,
            property,
            0,
        ),
        Expr::IndexGet { object, index } => {
            let owner = normalize(ctx.aliases, &infer_expr(ctx, locals, object, depth + 1)?);
            match owner {
                Type::Array(element) => Some(*element),
                Type::Tuple(elements) => match index.as_ref() {
                    Expr::Integer(index) => elements.get(usize::try_from(*index).ok()?).cloned(),
                    _ if !elements.is_empty()
                        && elements.windows(2).all(|pair| pair[0] == pair[1]) =>
                    {
                        elements.first().cloned()
                    }
                    _ => None,
                },
                _ => None,
            }
        }
        Expr::Array(elements) => {
            if elements.is_empty() {
                return Some(Type::Array(Box::new(Type::Never)));
            }
            let mut element_types = Vec::new();
            for element in elements {
                let ty = infer_expr(ctx, locals, element, depth + 1)?;
                if !element_types.contains(&ty) {
                    element_types.push(ty);
                }
            }
            let element = if element_types.len() == 1 {
                element_types.pop().unwrap()
            } else {
                Type::Union(element_types)
            };
            Some(Type::Array(Box::new(element)))
        }
        Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => {
            let class = ctx.classes.get(class_name)?;
            if class.fields.len() != args.len() {
                return None;
            }
            let mut properties = HashMap::new();
            let mut order = Vec::new();
            for (field, arg) in class.fields.iter().zip(args) {
                let ty = infer_expr(ctx, locals, arg, depth + 1)?;
                order.push(field.name.clone());
                properties.insert(
                    field.name.clone(),
                    PropertyInfo {
                        ty,
                        optional: false,
                        readonly: false,
                    },
                );
            }
            Some(Type::Object(ObjectType {
                name: None,
                properties,
                property_order: Some(order),
                index_signature: None,
            }))
        }
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            let then_ty = infer_expr(ctx, locals, then_expr, depth + 1)?;
            let else_ty = infer_expr(ctx, locals, else_expr, depth + 1)?;
            if assignable(ctx.aliases, &then_ty, &else_ty, 0) {
                Some(else_ty)
            } else if assignable(ctx.aliases, &else_ty, &then_ty, 0) {
                Some(then_ty)
            } else {
                Some(Type::Union(vec![then_ty, else_ty]))
            }
        }
        Expr::Call { callee, args, .. } => {
            let Expr::FuncRef(function_id) = callee.as_ref() else {
                return None;
            };
            if !call_is_proven(ctx, locals, *function_id, args) {
                return None;
            }
            Some(ctx.functions.get(function_id)?.return_type.clone())
        }
        Expr::Binary { op, left, right } => {
            use perry_hir::BinaryOp;
            let left = infer_expr(ctx, locals, left, depth + 1)?;
            let right = infer_expr(ctx, locals, right, depth + 1)?;
            if matches!(op, BinaryOp::Add)
                && (assignable(ctx.aliases, &left, &Type::String, 0)
                    || assignable(ctx.aliases, &right, &Type::String, 0))
            {
                Some(Type::String)
            } else if matches!(
                op,
                BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Mod
                    | BinaryOp::Pow
                    | BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr
                    | BinaryOp::UShr
            ) && assignable(ctx.aliases, &left, &Type::Number, 0)
                && assignable(ctx.aliases, &right, &Type::Number, 0)
            {
                Some(Type::Number)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn expr_proves(
    ctx: &ProofCtx<'_>,
    locals: &HashMap<u32, Type>,
    expr: &Expr,
    expected: &Type,
    depth: usize,
) -> bool {
    if depth > 64 {
        return false;
    }
    match (expr, normalize(ctx.aliases, expected)) {
        (Expr::Array(elements), Type::Array(expected_element)) => elements
            .iter()
            .all(|element| expr_proves(ctx, locals, element, &expected_element, depth + 1)),
        (
            Expr::Conditional {
                then_expr,
                else_expr,
                ..
            },
            expected,
        ) => {
            expr_proves(ctx, locals, then_expr, &expected, depth + 1)
                && expr_proves(ctx, locals, else_expr, &expected, depth + 1)
        }
        (_, expected) => infer_expr(ctx, locals, expr, depth + 1)
            .is_some_and(|actual| assignable(ctx.aliases, &actual, &expected, 0)),
    }
}

fn merge_locals(
    ctx: &ProofCtx<'_>,
    left: &HashMap<u32, Type>,
    right: &HashMap<u32, Type>,
) -> HashMap<u32, Type> {
    left.iter()
        .filter_map(|(id, left_ty)| {
            let right_ty = right.get(id)?;
            if left_ty == right_ty || assignable(ctx.aliases, right_ty, left_ty, 0) {
                Some((*id, left_ty.clone()))
            } else if assignable(ctx.aliases, left_ty, right_ty, 0) {
                Some((*id, right_ty.clone()))
            } else {
                None
            }
        })
        .collect()
}

struct SinglePassFlow {
    active: Vec<HashMap<u32, Type>>,
    exits: Vec<HashMap<u32, Type>>,
}

/// Path-sensitive verifier for the `do { ... break } while (false)` regions
/// emitted by destructuring/control-flow lowering. Treating `break` as an
/// ordinary statement loses the successful branch's facts; treating the last
/// assignment as dominant would be unsound. Enumerating these finite paths
/// keeps both sides exact without trying to solve general loop fixed points.
fn verify_single_pass_sequence(
    ctx: &ProofCtx<'_>,
    stmts: &[Stmt],
    mut active: Vec<HashMap<u32, Type>>,
    expected_return: &Type,
    found_return: &mut bool,
) -> Option<SinglePassFlow> {
    let mut exits = Vec::new();
    for stmt in stmts {
        let mut next = Vec::new();
        for mut locals in active {
            match stmt {
                Stmt::Let { id, ty, init, .. } => {
                    locals.remove(id);
                    if let Some(init) = init {
                        observe_expr_effects(ctx, &mut locals, init);
                        if expr_proves(ctx, &locals, init, ty, 0) {
                            locals.insert(*id, ty.clone());
                        } else if let Some(actual) = infer_expr(ctx, &locals, init, 0) {
                            locals.insert(*id, actual);
                        }
                    }
                    next.push(locals);
                }
                Stmt::Expr(Expr::LocalSet(id, value)) => {
                    observe_expr_effects(ctx, &mut locals, value);
                    update_local_from_expr(ctx, &mut locals, *id, value);
                    next.push(locals);
                }
                Stmt::Expr(expr) => {
                    observe_expr_effects(ctx, &mut locals, expr);
                    next.push(locals);
                }
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    observe_expr_effects(ctx, &mut locals, condition);
                    let then_flow = verify_single_pass_sequence(
                        ctx,
                        then_branch,
                        vec![locals.clone()],
                        expected_return,
                        found_return,
                    )?;
                    next.extend(then_flow.active);
                    exits.extend(then_flow.exits);
                    if let Some(else_branch) = else_branch {
                        let else_flow = verify_single_pass_sequence(
                            ctx,
                            else_branch,
                            vec![locals],
                            expected_return,
                            found_return,
                        )?;
                        next.extend(else_flow.active);
                        exits.extend(else_flow.exits);
                    } else {
                        next.push(locals);
                    }
                }
                Stmt::Break | Stmt::Continue => exits.push(locals),
                Stmt::Return(Some(value)) => {
                    *found_return = true;
                    observe_expr_effects(ctx, &mut locals, value);
                    if !expr_proves(ctx, &locals, value, expected_return, 0) {
                        return None;
                    }
                }
                Stmt::Throw(_) => {}
                // Keep this deliberately specific to the finite lowering
                // shape. Nested loops, labels, switches and exception joins
                // stay outside the optional return-fact set.
                _ => return None,
            }
        }
        active = next;
    }
    Some(SinglePassFlow { active, exits })
}

fn merge_paths(
    ctx: &ProofCtx<'_>,
    mut paths: impl Iterator<Item = HashMap<u32, Type>>,
) -> Option<HashMap<u32, Type>> {
    let mut merged = paths.next()?;
    for path in paths {
        merged = merge_locals(ctx, &merged, &path);
    }
    Some(merged)
}

fn update_local_from_expr(
    ctx: &ProofCtx<'_>,
    locals: &mut HashMap<u32, Type>,
    id: u32,
    value: &Expr,
) {
    if let Some(current) = locals.get(&id).cloned() {
        if expr_proves(ctx, locals, value, &current, 0) {
            return;
        }
    }
    if let Some(actual) = infer_expr(ctx, locals, value, 0) {
        locals.insert(id, actual);
    } else {
        locals.remove(&id);
    }
}

fn root_local(expr: &Expr) -> Option<u32> {
    match expr {
        Expr::LocalGet(id) => Some(*id),
        Expr::PropertyGet { object, .. } | Expr::IndexGet { object, .. } => root_local(object),
        _ => None,
    }
}

fn is_reference_like(aliases: &HashMap<String, Type>, ty: &Type, depth: usize) -> bool {
    if depth > 32 {
        return true;
    }
    match ty {
        Type::Named(name) => aliases
            .get(name)
            .map_or(true, |ty| is_reference_like(aliases, ty, depth + 1)),
        Type::Array(_) | Type::Tuple(_) | Type::Object(_) | Type::Generic { .. } => true,
        Type::Union(variants) => variants
            .iter()
            .any(|ty| is_reference_like(aliases, ty, depth + 1)),
        _ => false,
    }
}

fn invalidate_references_used_by(ctx: &ProofCtx<'_>, locals: &mut HashMap<u32, Type>, expr: &Expr) {
    let mut escaped = HashSet::new();
    fn collect(
        ctx: &ProofCtx<'_>,
        locals: &HashMap<u32, Type>,
        expr: &Expr,
        escaped: &mut HashSet<u32>,
    ) {
        if let Expr::LocalGet(id) = expr {
            if locals
                .get(id)
                .is_some_and(|ty| is_reference_like(ctx.aliases, ty, 0))
            {
                escaped.insert(*id);
            }
        }
        perry_hir::walker::walk_expr_children(expr, &mut |child| {
            collect(ctx, locals, child, escaped)
        });
    }
    collect(ctx, locals, expr, &mut escaped);
    if !escaped.is_empty() {
        // Different static object types can still alias through structural
        // typing. Once one reference escapes to unknown code, retain no
        // reference proof that might describe the same object graph.
        locals.retain(|id, ty| !escaped.contains(id) && !is_reference_like(ctx.aliases, ty, 0));
    }
}

/// Apply effects that can invalidate facts established by the entry guard.
/// Calls to another constructively verified guarded clone are safe: its plan
/// excludes mutated/captured parameters, and the fixed point removes callees
/// that themselves let a proof reference escape to unknown code.
fn observe_expr_effects(ctx: &ProofCtx<'_>, locals: &mut HashMap<u32, Type>, expr: &Expr) {
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        observe_expr_effects(ctx, locals, child)
    });

    if let Some((root, preserves)) = mutation_preserves_proof(ctx, locals, expr) {
        if !preserves {
            invalidate_local_and_type_aliases(ctx, locals, root);
        }
    }

    match expr {
        Expr::Call { callee, args, .. } => {
            let proven = match callee.as_ref() {
                Expr::FuncRef(id) => call_is_proven(ctx, locals, *id, args),
                _ => false,
            };
            if !proven {
                invalidate_references_used_by(ctx, locals, callee);
                for arg in args {
                    invalidate_references_used_by(ctx, locals, arg);
                }
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(object) = object {
                invalidate_references_used_by(ctx, locals, object);
            }
            for arg in args {
                invalidate_references_used_by(ctx, locals, arg);
            }
        }
        Expr::New {
            class_name, args, ..
        } if !class_name.starts_with("__AnonShape_") => {
            for arg in args {
                invalidate_references_used_by(ctx, locals, arg);
            }
        }
        Expr::NewDynamic { callee, args, .. } => {
            invalidate_references_used_by(ctx, locals, callee);
            for arg in args {
                invalidate_references_used_by(ctx, locals, arg);
            }
        }
        Expr::ObjectAssign { target, sources } => {
            invalidate_references_used_by(ctx, locals, target);
            for source in sources {
                invalidate_references_used_by(ctx, locals, source);
            }
        }
        _ => {}
    }
}

fn mutation_preserves_proof(
    ctx: &ProofCtx<'_>,
    locals: &HashMap<u32, Type>,
    expr: &Expr,
) -> Option<(u32, bool)> {
    match expr {
        Expr::PropertySet {
            object,
            property,
            value,
        } => {
            let root = root_local(object)?;
            let expected = infer_expr(ctx, locals, object, 0)
                .and_then(|owner| property_type(ctx, &owner, property, 0));
            Some((
                root,
                expected.is_some_and(|expected| expr_proves(ctx, locals, value, &expected, 0)),
            ))
        }
        Expr::IndexSet { object, value, .. } => {
            let root = root_local(object)?;
            let expected = infer_expr(ctx, locals, object, 0).and_then(|owner| {
                match normalize(ctx.aliases, &owner) {
                    Type::Array(element) => Some(*element),
                    _ => None,
                }
            });
            Some((
                root,
                expected.is_some_and(|expected| expr_proves(ctx, locals, value, &expected, 0)),
            ))
        }
        Expr::PutValueSet { target, value, .. } => {
            let root = root_local(target)?;
            let expected = infer_expr(ctx, locals, target, 0).and_then(|target| {
                match normalize(ctx.aliases, &target) {
                    Type::Array(element) => Some(*element),
                    _ => None,
                }
            });
            Some((
                root,
                expected.is_some_and(|expected| expr_proves(ctx, locals, value, &expected, 0)),
            ))
        }
        Expr::PropertyUpdate {
            object, property, ..
        } => {
            let root = root_local(object)?;
            let numeric = infer_expr(ctx, locals, object, 0)
                .and_then(|owner| property_type(ctx, &owner, property, 0))
                .is_some_and(|ty| assignable(ctx.aliases, &ty, &Type::Number, 0));
            Some((root, numeric))
        }
        Expr::IndexUpdate { object, .. } => {
            let root = root_local(object)?;
            let numeric = infer_expr(ctx, locals, object, 0).is_some_and(|owner| {
                matches!(normalize(ctx.aliases, &owner), Type::Array(element) if assignable(ctx.aliases, &element, &Type::Number, 0))
            });
            Some((root, numeric))
        }
        _ => None,
    }
}

fn invalidate_local_and_type_aliases(
    ctx: &ProofCtx<'_>,
    locals: &mut HashMap<u32, Type>,
    root: u32,
) {
    let Some(ty) = locals.get(&root).cloned() else {
        return;
    };
    if is_reference_like(ctx.aliases, &ty, 0) {
        // Structural typing permits differently annotated locals to alias the
        // same graph, so a failed preserving-write proof invalidates all
        // reference facts, not just equal `Type` values.
        locals.retain(|_, candidate| !is_reference_like(ctx.aliases, candidate, 0));
    } else {
        locals.remove(&root);
    }
}

fn verify_block(
    ctx: &ProofCtx<'_>,
    stmts: &[Stmt],
    locals: &mut HashMap<u32, Type>,
    expected_return: &Type,
    found_return: &mut bool,
) -> bool {
    for stmt in stmts {
        match stmt {
            Stmt::Let { id, ty, init, .. } => {
                locals.remove(id);
                let Some(init) = init else {
                    continue;
                };
                observe_expr_effects(ctx, locals, init);
                if expr_proves(ctx, locals, init, ty, 0) {
                    locals.insert(*id, ty.clone());
                } else if let Some(actual) = infer_expr(ctx, locals, init, 0) {
                    locals.insert(*id, actual);
                }
            }
            Stmt::Expr(Expr::LocalSet(id, value)) => {
                observe_expr_effects(ctx, locals, value);
                update_local_from_expr(ctx, locals, *id, value);
            }
            Stmt::Expr(expr) => {
                observe_expr_effects(ctx, locals, expr);
            }
            Stmt::Return(Some(value)) => {
                *found_return = true;
                observe_expr_effects(ctx, locals, value);
                if !expr_proves(ctx, locals, value, expected_return, 0) {
                    return false;
                }
            }
            Stmt::Return(None) => return false,
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                observe_expr_effects(ctx, locals, condition);
                let before = locals.clone();
                let mut then_locals = before.clone();
                if !verify_block(
                    ctx,
                    then_branch,
                    &mut then_locals,
                    expected_return,
                    found_return,
                ) {
                    return false;
                }
                let mut else_locals = before;
                if let Some(else_branch) = else_branch {
                    if !verify_block(
                        ctx,
                        else_branch,
                        &mut else_locals,
                        expected_return,
                        found_return,
                    ) {
                        return false;
                    }
                }
                *locals = merge_locals(ctx, &then_locals, &else_locals);
            }
            Stmt::DoWhile {
                body,
                condition: Expr::Bool(false),
            } => {
                let flow = verify_single_pass_sequence(
                    ctx,
                    body,
                    vec![locals.clone()],
                    expected_return,
                    found_return,
                );
                let Some(flow) = flow else {
                    return false;
                };
                let Some(merged) = merge_paths(ctx, flow.active.into_iter().chain(flow.exits))
                else {
                    // Every path returns or throws; the remainder is
                    // unreachable, but declining the optional fact is simpler
                    // than threading reachability through the outer verifier.
                    return false;
                };
                *locals = merged;
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                observe_expr_effects(ctx, locals, condition);
                let before = locals.clone();
                let mut body_locals = before.clone();
                if !verify_block(ctx, body, &mut body_locals, expected_return, found_return) {
                    return false;
                }
                *locals = merge_locals(ctx, &before, &body_locals);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                let mut loop_locals = locals.clone();
                if let Some(init) = init {
                    if !verify_block(
                        ctx,
                        std::slice::from_ref(init.as_ref()),
                        &mut loop_locals,
                        expected_return,
                        found_return,
                    ) {
                        return false;
                    }
                }
                if let Some(condition) = condition {
                    observe_expr_effects(ctx, &mut loop_locals, condition);
                }
                if !verify_block(ctx, body, &mut loop_locals, expected_return, found_return) {
                    return false;
                }
                if let Some(update) = update {
                    observe_expr_effects(ctx, &mut loop_locals, update);
                }
                *locals = merge_locals(ctx, locals, &loop_locals);
            }
            Stmt::Labeled { body, .. } => {
                if !verify_block(
                    ctx,
                    std::slice::from_ref(body.as_ref()),
                    locals,
                    expected_return,
                    found_return,
                ) {
                    return false;
                }
            }
            Stmt::Throw(_)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_) => {}
            // Return-proof facts are optional. Complex exceptional/fallthrough
            // joins remain generic until their proof can be modeled exactly.
            Stmt::Try { .. } | Stmt::Switch { .. } => return false,
        }
    }
    true
}

fn verify_function(ctx: &ProofCtx<'_>, function: &Function) -> bool {
    if function.is_async || function.is_generator || function.was_plain_async {
        return false;
    }
    match function.body.last() {
        Some(Stmt::Return(Some(_))) | Some(Stmt::Throw(_)) => {}
        _ => return false,
    }
    let Some(plan) = ctx.plans.get(&function.id) else {
        return false;
    };
    let mut locals = plan_param_proofs(function, plan);
    let mut found_return = false;
    verify_block(
        ctx,
        &function.body,
        &mut locals,
        &function.return_type,
        &mut found_return,
    ) && found_return
}

pub(crate) fn collect_proven_returns(
    hir: &Module,
    plans: &HashMap<u32, SpecFnPlan>,
    aliases: &HashMap<String, Type>,
) -> HashMap<u32, Type> {
    let functions: HashMap<u32, &Function> = hir.functions.iter().map(|f| (f.id, f)).collect();
    let classes: HashMap<String, &Class> =
        hir.classes.iter().map(|c| (c.name.clone(), c)).collect();
    let mut candidates: HashSet<u32> = plans.keys().copied().collect();

    loop {
        let snapshot = candidates.clone();
        let ctx = ProofCtx {
            functions: functions.clone(),
            classes: classes.clone(),
            plans,
            aliases,
            candidates: &snapshot,
        };
        candidates.retain(|id| {
            functions
                .get(id)
                .is_some_and(|function| verify_function(&ctx, function))
        });
        if candidates == snapshot {
            break;
        }
    }

    candidates
        .into_iter()
        .filter_map(|id| {
            functions
                .get(&id)
                .map(|function| (id, function.return_type.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::spec_abi::SpecParamGuard;
    use crate::codegen::SpecDispatch;
    use perry_hir::{Param, TypeAlias};

    fn function(id: u32, name: &str, body: Vec<Stmt>, payload: &Type) -> Function {
        Function {
            id,
            name: name.to_string(),
            type_params: Vec::new(),
            params: vec![Param {
                id: id * 10,
                name: "value".to_string(),
                ty: payload.clone(),
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            }],
            return_type: payload.clone(),
            body,
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }
    }

    fn plan(payload: &Type) -> SpecFnPlan {
        SpecFnPlan {
            reps: vec![SpecParamRep::Boxed],
            dispatch: SpecDispatch::Guarded,
            guards: vec![Some(SpecParamGuard {
                proof: payload.clone(),
                descriptor_name: "test_guard".to_string(),
                descriptor: vec![1],
            })],
        }
    }

    #[test]
    fn only_constructively_verified_returns_propagate_through_calls() {
        let payload = Type::Object(ObjectType {
            name: Some("Payload".to_string()),
            properties: HashMap::from([
                (
                    "label".to_string(),
                    PropertyInfo {
                        ty: Type::String,
                        optional: false,
                        readonly: false,
                    },
                ),
                (
                    "count".to_string(),
                    PropertyInfo {
                        ty: Type::Number,
                        optional: false,
                        readonly: false,
                    },
                ),
            ]),
            property_order: Some(vec!["label".to_string(), "count".to_string()]),
            index_signature: None,
        });
        let identity = function(
            1,
            "identity",
            vec![Stmt::Return(Some(Expr::LocalGet(10)))],
            &payload,
        );
        let forward = function(
            2,
            "forward",
            vec![
                Stmt::Let {
                    id: 99,
                    name: "result".to_string(),
                    ty: payload.clone(),
                    mutable: false,
                    init: Some(Expr::Call {
                        callee: Box::new(Expr::FuncRef(1)),
                        args: vec![Expr::LocalGet(20)],
                        type_args: Vec::new(),
                        byte_offset: 0,
                    }),
                },
                Stmt::Return(Some(Expr::LocalGet(99))),
            ],
            &payload,
        );
        let liar = function(
            3,
            "liar",
            vec![Stmt::Return(Some(Expr::Undefined))],
            &payload,
        );
        let escaping = function(
            4,
            "escaping",
            vec![
                Stmt::Expr(Expr::Call {
                    callee: Box::new(Expr::ExternFuncRef {
                        name: "unknown".to_string(),
                        param_types: vec![payload.clone()],
                        return_type: Type::Void,
                    }),
                    args: vec![Expr::LocalGet(40)],
                    type_args: Vec::new(),
                    byte_offset: 0,
                }),
                Stmt::Return(Some(Expr::LocalGet(40))),
            ],
            &payload,
        );
        let one_pass_number = Function {
            id: 5,
            name: "onePassNumber".to_string(),
            type_params: Vec::new(),
            params: vec![Param {
                id: 50,
                name: "value".to_string(),
                ty: payload.clone(),
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            }],
            return_type: Type::Number,
            body: vec![
                Stmt::Let {
                    id: 51,
                    name: "number".to_string(),
                    ty: Type::Number,
                    mutable: true,
                    init: Some(Expr::Undefined),
                },
                Stmt::DoWhile {
                    body: vec![
                        Stmt::If {
                            condition: Expr::Bool(true),
                            then_branch: vec![
                                Stmt::Expr(Expr::LocalSet(
                                    51,
                                    Box::new(Expr::PropertyGet {
                                        object: Box::new(Expr::LocalGet(50)),
                                        property: "count".to_string(),
                                        byte_offset: 0,
                                    }),
                                )),
                                Stmt::Break,
                            ],
                            else_branch: None,
                        },
                        Stmt::Expr(Expr::LocalSet(51, Box::new(Expr::Integer(0)))),
                        Stmt::Break,
                    ],
                    condition: Expr::Bool(false),
                },
                Stmt::Return(Some(Expr::LocalGet(51))),
            ],
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        };
        let mut module = Module::new("return_proof.ts");
        module.functions = vec![identity, forward, liar, escaping, one_pass_number];
        module.type_aliases.push(TypeAlias {
            id: 1,
            name: "Payload".to_string(),
            type_params: Vec::new(),
            ty: payload.clone(),
            is_exported: false,
        });
        let plans = HashMap::from([
            (1, plan(&payload)),
            (2, plan(&payload)),
            (3, plan(&payload)),
            (4, plan(&payload)),
            (5, plan(&payload)),
        ]);
        let proofs = collect_proven_returns(&module, &plans, &HashMap::new());
        assert!(proofs.contains_key(&1));
        assert!(proofs.contains_key(&2));
        assert!(!proofs.contains_key(&3));
        assert!(!proofs.contains_key(&4));
        assert!(proofs.contains_key(&5));
    }
}
