//! Scalar replacement for fixed aggregate literals exposed by inlining.
//!
//! The ordinary codegen escape pass handles `const point = { x, y }` and
//! `const values = [x, y]`, but an ECS-style helper exposes a nested shape:
//!
//! ```text
//! const arg = [{ component: Position }, { component: Velocity }];
//! const item = arg[0];
//! read(item.component);
//! ```
//!
//! Once a known helper has been inlined and its short loop unrolled, neither
//! carrier identity is observable. This pass replaces every object field with
//! a synthetic scalar local, rewrites the proven field reads, and removes the
//! carrier array and element aliases. Any identity, mutation, reflection,
//! closure capture, dynamic index, missing/inherited property, or method-call
//! receiver use rejects the whole candidate and leaves normal materialization
//! intact.

use std::collections::{HashMap, HashSet};

use perry_hir::types::{LocalId, Type};
use perry_hir::{Expr, Module, Stmt};

const MAX_SCALAR_AGGREGATE_LEN: usize = 8;
const MAX_SCALAR_AGGREGATE_FIELDS: usize = 16;

type AnonShapeFields = HashMap<String, Vec<String>>;

pub fn run(module: &mut Module) {
    let mut next_local_id = crate::generator::compute_max_local_id(module).saturating_add(1);
    let mut source_span_remaps = Vec::new();
    // Closed-shape object literals are represented as `new __AnonShape_*`
    // before transforms run. Constructor parameter names retain the literal's
    // source field order, while the call arguments retain its value order.
    let anon_shape_fields: AnonShapeFields = module
        .classes
        .iter()
        .filter(|class| class.name.starts_with("__AnonShape_"))
        .filter_map(|class| {
            let constructor = class.constructor.as_ref()?;
            (!constructor.params.is_empty()).then(|| {
                (
                    class.name.clone(),
                    constructor
                        .params
                        .iter()
                        .map(|param| param.name.clone())
                        .collect(),
                )
            })
        })
        .collect();

    scalarize_stmts(
        &mut module.init,
        &mut next_local_id,
        &mut source_span_remaps,
        &anon_shape_fields,
    );
    for function in &mut module.functions {
        scalarize_stmts(
            &mut function.body,
            &mut next_local_id,
            &mut source_span_remaps,
            &anon_shape_fields,
        );
    }
    for class in &mut module.classes {
        if let Some(constructor) = &mut class.constructor {
            scalarize_stmts(
                &mut constructor.body,
                &mut next_local_id,
                &mut source_span_remaps,
                &anon_shape_fields,
            );
        }
        for method in &mut class.methods {
            scalarize_stmts(
                &mut method.body,
                &mut next_local_id,
                &mut source_span_remaps,
                &anon_shape_fields,
            );
        }
        for (_, getter) in &mut class.getters {
            scalarize_stmts(
                &mut getter.body,
                &mut next_local_id,
                &mut source_span_remaps,
                &anon_shape_fields,
            );
        }
        for (_, setter) in &mut class.setters {
            scalarize_stmts(
                &mut setter.body,
                &mut next_local_id,
                &mut source_span_remaps,
                &anon_shape_fields,
            );
        }
        for method in &mut class.static_methods {
            scalarize_stmts(
                &mut method.body,
                &mut next_local_id,
                &mut source_span_remaps,
                &anon_shape_fields,
            );
        }
    }

    for (source_id, new_id) in source_span_remaps {
        if let Some(span) = module.local_source_spans.get(&source_id).copied() {
            module.local_source_spans.insert(new_id, span);
        }
    }
}

fn scalarize_stmts(
    stmts: &mut Vec<Stmt>,
    next_local_id: &mut LocalId,
    source_span_remaps: &mut Vec<(LocalId, LocalId)>,
    anon_shape_fields: &AnonShapeFields,
) {
    let candidates: Vec<LocalId> = stmts
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::Let {
                id,
                mutable: false,
                init: Some(Expr::Array(elements)),
                ..
            } if aggregate_elements_are_plain(elements, anon_shape_fields) => Some(*id),
            _ => None,
        })
        .collect();

    for array_id in candidates {
        let _ = scalarize_candidate(
            stmts,
            array_id,
            next_local_id,
            source_span_remaps,
            anon_shape_fields,
        );
    }

    // A candidate created inside a branch/loop belongs to that nested lexical
    // statement list, so process child lists after the current scope.
    for stmt in stmts {
        match stmt {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                scalarize_stmts(
                    then_branch,
                    next_local_id,
                    source_span_remaps,
                    anon_shape_fields,
                );
                if let Some(else_branch) = else_branch {
                    scalarize_stmts(
                        else_branch,
                        next_local_id,
                        source_span_remaps,
                        anon_shape_fields,
                    );
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                scalarize_stmts(body, next_local_id, source_span_remaps, anon_shape_fields);
            }
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    let mut init_vec = vec![(**init).clone()];
                    scalarize_stmts(
                        &mut init_vec,
                        next_local_id,
                        source_span_remaps,
                        anon_shape_fields,
                    );
                    if init_vec.len() == 1 {
                        **init = init_vec.remove(0);
                    }
                }
                scalarize_stmts(body, next_local_id, source_span_remaps, anon_shape_fields);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                scalarize_stmts(body, next_local_id, source_span_remaps, anon_shape_fields);
                if let Some(catch) = catch {
                    scalarize_stmts(
                        &mut catch.body,
                        next_local_id,
                        source_span_remaps,
                        anon_shape_fields,
                    );
                }
                if let Some(finally) = finally {
                    scalarize_stmts(
                        finally,
                        next_local_id,
                        source_span_remaps,
                        anon_shape_fields,
                    );
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    scalarize_stmts(
                        &mut case.body,
                        next_local_id,
                        source_span_remaps,
                        anon_shape_fields,
                    );
                }
            }
            // A labeled body is one statement rather than a statement list.
            // Aggregate-call inlining never creates this shape; keep it on the
            // conservative materialized path instead of inventing a wrapper.
            Stmt::Labeled { .. }
            | Stmt::Let { .. }
            | Stmt::Expr(_)
            | Stmt::Return(_)
            | Stmt::Throw(_)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
        }
    }
}

fn element_properties(
    element: &Expr,
    anon_shape_fields: &AnonShapeFields,
) -> Option<Vec<(String, Expr)>> {
    match element {
        Expr::Object(properties) => Some(properties.clone()),
        Expr::New {
            class_name,
            args,
            cap_args_appended: 0,
            ..
        } if class_name.starts_with("__AnonShape_") => {
            let fields = anon_shape_fields.get(class_name)?;
            (fields.len() == args.len())
                .then(|| fields.iter().cloned().zip(args.iter().cloned()).collect())
        }
        _ => None,
    }
}

fn aggregate_elements_are_plain(elements: &[Expr], anon_shape_fields: &AnonShapeFields) -> bool {
    !elements.is_empty()
        && elements.len() <= MAX_SCALAR_AGGREGATE_LEN
        && elements.iter().all(|element| {
            element_properties(element, anon_shape_fields).is_some_and(|properties| {
                !properties.is_empty()
                    && properties.len() <= MAX_SCALAR_AGGREGATE_FIELDS
                    && properties.iter().all(|(key, value)| {
                        key != "__proto__"
                            && !matches!(
                                value,
                                Expr::Closure {
                                    captures_this: true,
                                    ..
                                }
                            )
                    })
            })
        })
}

fn const_index(expr: &Expr) -> Option<usize> {
    match expr {
        Expr::Integer(value) if *value >= 0 => usize::try_from(*value).ok(),
        Expr::Number(value)
            if value.is_finite()
                && *value >= 0.0
                && value.fract() == 0.0
                && *value <= usize::MAX as f64 =>
        {
            Some(*value as usize)
        }
        _ => None,
    }
}

fn scalarize_candidate(
    stmts: &mut Vec<Stmt>,
    array_id: LocalId,
    next_local_id: &mut LocalId,
    source_span_remaps: &mut Vec<(LocalId, LocalId)>,
    anon_shape_fields: &AnonShapeFields,
) -> bool {
    let Some(elements) = stmts.iter().find_map(|stmt| match stmt {
        Stmt::Let {
            id,
            init: Some(Expr::Array(elements)),
            ..
        } if *id == array_id => Some(elements.clone()),
        _ => None,
    }) else {
        return false;
    };
    if !aggregate_elements_are_plain(&elements, anon_shape_fields) {
        return false;
    }

    let properties: Vec<Vec<(String, Expr)>> = elements
        .iter()
        .map(|element| element_properties(element, anon_shape_fields))
        .collect::<Option<_>>()
        .expect("aggregate_elements_are_plain checked the shape");

    let keys: Vec<HashSet<String>> = properties
        .iter()
        .map(|properties| properties.iter().map(|(key, _)| key.clone()).collect())
        .collect();
    let mut aliases = HashMap::new();
    collect_aliases(stmts, array_id, elements.len(), &mut aliases);
    if !stmts_are_safe(stmts, array_id, &aliases, &keys) {
        return false;
    }

    let mut scalar_lets = Vec::new();
    let mut fields: Vec<HashMap<String, LocalId>> = Vec::with_capacity(elements.len());
    for (element_index, properties) in properties.into_iter().enumerate() {
        let mut element_fields = HashMap::new();
        for (property_index, (key, value)) in properties.into_iter().enumerate() {
            let id = *next_local_id;
            *next_local_id = next_local_id.saturating_add(1);
            source_span_remaps.push((array_id, id));
            scalar_lets.push(Stmt::Let {
                id,
                name: format!(
                    "__perry_scalar_aggregate_{array_id}_{element_index}_{property_index}"
                ),
                ty: Type::Any,
                mutable: false,
                init: Some(value),
            });
            // Object literal duplicate keys are last-write-wins, while every
            // value expression above is still evaluated in source order.
            element_fields.insert(key, id);
        }
        fields.push(element_fields);
    }

    rewrite_stmts(stmts, array_id, &aliases, &fields);
    let Some(declaration_index) = stmts
        .iter()
        .position(|stmt| matches!(stmt, Stmt::Let { id, .. } if *id == array_id))
    else {
        return false;
    };
    stmts.splice(declaration_index..=declaration_index, scalar_lets);
    true
}

fn collect_aliases(
    stmts: &[Stmt],
    array_id: LocalId,
    len: usize,
    aliases: &mut HashMap<LocalId, usize>,
) {
    for stmt in stmts {
        if let Stmt::Let {
            id,
            mutable: false,
            init: Some(Expr::IndexGet { object, index }),
            ..
        } = stmt
        {
            if matches!(object.as_ref(), Expr::LocalGet(candidate) if *candidate == array_id) {
                if let Some(index) = const_index(index).filter(|index| *index < len) {
                    aliases.insert(*id, index);
                }
            }
        }
        match stmt {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_aliases(then_branch, array_id, len, aliases);
                if let Some(else_branch) = else_branch {
                    collect_aliases(else_branch, array_id, len, aliases);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                collect_aliases(body, array_id, len, aliases);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_aliases(body, array_id, len, aliases);
                if let Some(catch) = catch {
                    collect_aliases(&catch.body, array_id, len, aliases);
                }
                if let Some(finally) = finally {
                    collect_aliases(finally, array_id, len, aliases);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_aliases(&case.body, array_id, len, aliases);
                }
            }
            _ => {}
        }
    }
}

fn candidate_field(
    object: &Expr,
    property: &str,
    array_id: LocalId,
    aliases: &HashMap<LocalId, usize>,
    fields: &[HashSet<String>],
) -> Option<usize> {
    let index = match object {
        Expr::LocalGet(alias) => aliases.get(alias).copied(),
        Expr::IndexGet { object, index } if matches!(object.as_ref(), Expr::LocalGet(id) if *id == array_id) => {
            const_index(index)
        }
        _ => None,
    }?;
    fields
        .get(index)
        .is_some_and(|element| element.contains(property))
        .then_some(index)
}

fn expr_is_safe(
    expr: &Expr,
    array_id: LocalId,
    aliases: &HashMap<LocalId, usize>,
    fields: &[HashSet<String>],
) -> bool {
    match expr {
        Expr::PropertyGet {
            object, property, ..
        } => {
            if candidate_field(object, property, array_id, aliases, fields).is_some() {
                return true;
            }
            if property == "length"
                && matches!(object.as_ref(), Expr::LocalGet(id) if *id == array_id)
            {
                return true;
            }
        }
        Expr::IndexGet { object, .. } if matches!(object.as_ref(), Expr::LocalGet(id) if *id == array_id) =>
        {
            // Only an alias declaration or a direct property receiver is safe;
            // both parents intercept this expression before recursion reaches it.
            return false;
        }
        Expr::LocalGet(id) if *id == array_id || aliases.contains_key(id) => return false,
        Expr::LocalSet(id, _) | Expr::Update { id, .. }
            if *id == array_id || aliases.contains_key(id) =>
        {
            return false;
        }
        Expr::Closure {
            captures,
            mutable_captures,
            ..
        } if captures
            .iter()
            .chain(mutable_captures.iter())
            .any(|id| *id == array_id || aliases.contains_key(id)) =>
        {
            return false;
        }
        Expr::Call { callee, .. } | Expr::CallSpread { callee, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { object, property, .. }
                    if candidate_field(object, property, array_id, aliases, fields).is_some()
            ) =>
        {
            // `item.method()` observes the original object as `this`.
            return false;
        }
        Expr::Delete(operand)
            if matches!(
                operand.as_ref(),
                Expr::PropertyGet { object, property, .. }
                    if candidate_field(object, property, array_id, aliases, fields).is_some()
            ) =>
        {
            return false;
        }
        _ => {}
    }

    let mut safe = true;
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        safe &= expr_is_safe(child, array_id, aliases, fields);
    });
    safe
}

fn stmts_are_safe(
    stmts: &[Stmt],
    array_id: LocalId,
    aliases: &HashMap<LocalId, usize>,
    fields: &[HashSet<String>],
) -> bool {
    for stmt in stmts {
        let safe = match stmt {
            Stmt::Let { id, init, .. } if *id == array_id => true,
            Stmt::Let {
                id,
                init: Some(Expr::IndexGet { object, index }),
                ..
            } if aliases.contains_key(id)
                && matches!(object.as_ref(), Expr::LocalGet(candidate) if *candidate == array_id)
                && const_index(index) == aliases.get(id).copied() =>
            {
                true
            }
            Stmt::Let { init, .. } => init
                .as_ref()
                .is_none_or(|expr| expr_is_safe(expr, array_id, aliases, fields)),
            Stmt::Expr(expr) | Stmt::Throw(expr) => expr_is_safe(expr, array_id, aliases, fields),
            Stmt::Return(value) => value
                .as_ref()
                .is_none_or(|expr| expr_is_safe(expr, array_id, aliases, fields)),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                expr_is_safe(condition, array_id, aliases, fields)
                    && stmts_are_safe(then_branch, array_id, aliases, fields)
                    && else_branch
                        .as_deref()
                        .is_none_or(|branch| stmts_are_safe(branch, array_id, aliases, fields))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                expr_is_safe(condition, array_id, aliases, fields)
                    && stmts_are_safe(body, array_id, aliases, fields)
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_deref().is_none_or(|init| {
                    stmts_are_safe(std::slice::from_ref(init), array_id, aliases, fields)
                }) && condition
                    .as_ref()
                    .is_none_or(|expr| expr_is_safe(expr, array_id, aliases, fields))
                    && update
                        .as_ref()
                        .is_none_or(|expr| expr_is_safe(expr, array_id, aliases, fields))
                    && stmts_are_safe(body, array_id, aliases, fields)
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                stmts_are_safe(body, array_id, aliases, fields)
                    && catch
                        .as_ref()
                        .is_none_or(|catch| stmts_are_safe(&catch.body, array_id, aliases, fields))
                    && finally
                        .as_deref()
                        .is_none_or(|body| stmts_are_safe(body, array_id, aliases, fields))
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                expr_is_safe(discriminant, array_id, aliases, fields)
                    && cases.iter().all(|case| {
                        case.test
                            .as_ref()
                            .is_none_or(|test| expr_is_safe(test, array_id, aliases, fields))
                            && stmts_are_safe(&case.body, array_id, aliases, fields)
                    })
            }
            Stmt::Labeled { body, .. } => stmts_are_safe(
                std::slice::from_ref(body.as_ref()),
                array_id,
                aliases,
                fields,
            ),
            Stmt::PreallocateBoxes(ids)
            | Stmt::PreallocateTdzBoxes(ids)
            | Stmt::ReleaseBoxes(ids) => !ids
                .iter()
                .any(|id| *id == array_id || aliases.contains_key(id)),
            Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => true,
        };
        if !safe {
            return false;
        }
    }
    true
}

fn replacement_for_expr(
    expr: &Expr,
    array_id: LocalId,
    aliases: &HashMap<LocalId, usize>,
    fields: &[HashMap<String, LocalId>],
) -> Option<Expr> {
    let Expr::PropertyGet {
        object, property, ..
    } = expr
    else {
        return None;
    };
    if property == "length" && matches!(object.as_ref(), Expr::LocalGet(id) if *id == array_id) {
        return Some(Expr::Integer(fields.len() as i64));
    }
    let index = match object.as_ref() {
        Expr::LocalGet(alias) => aliases.get(alias).copied(),
        Expr::IndexGet { object, index } if matches!(object.as_ref(), Expr::LocalGet(id) if *id == array_id) => {
            const_index(index)
        }
        _ => None,
    }?;
    fields
        .get(index)?
        .get(property)
        .copied()
        .map(Expr::LocalGet)
}

fn rewrite_expr(
    expr: &mut Expr,
    array_id: LocalId,
    aliases: &HashMap<LocalId, usize>,
    fields: &[HashMap<String, LocalId>],
) {
    if let Some(replacement) = replacement_for_expr(expr, array_id, aliases, fields) {
        *expr = replacement;
        return;
    }
    perry_hir::walker::walk_expr_children_mut(expr, &mut |child| {
        rewrite_expr(child, array_id, aliases, fields)
    });
}

fn rewrite_stmts(
    stmts: &mut Vec<Stmt>,
    array_id: LocalId,
    aliases: &HashMap<LocalId, usize>,
    fields: &[HashMap<String, LocalId>],
) {
    let mut index = 0;
    while index < stmts.len() {
        if matches!(&stmts[index], Stmt::Let { id, .. } if aliases.contains_key(id)) {
            stmts.remove(index);
            continue;
        }
        match &mut stmts[index] {
            Stmt::Let { init, .. } => {
                if let Some(init) = init {
                    rewrite_expr(init, array_id, aliases, fields);
                }
            }
            Stmt::Expr(expr) | Stmt::Throw(expr) => rewrite_expr(expr, array_id, aliases, fields),
            Stmt::Return(value) => {
                if let Some(value) = value {
                    rewrite_expr(value, array_id, aliases, fields);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                rewrite_expr(condition, array_id, aliases, fields);
                rewrite_stmts(then_branch, array_id, aliases, fields);
                if let Some(else_branch) = else_branch {
                    rewrite_stmts(else_branch, array_id, aliases, fields);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                rewrite_expr(condition, array_id, aliases, fields);
                rewrite_stmts(body, array_id, aliases, fields);
            }
            Stmt::For {
                condition,
                update,
                body,
                ..
            } => {
                if let Some(condition) = condition {
                    rewrite_expr(condition, array_id, aliases, fields);
                }
                if let Some(update) = update {
                    rewrite_expr(update, array_id, aliases, fields);
                }
                rewrite_stmts(body, array_id, aliases, fields);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                rewrite_stmts(body, array_id, aliases, fields);
                if let Some(catch) = catch {
                    rewrite_stmts(&mut catch.body, array_id, aliases, fields);
                }
                if let Some(finally) = finally {
                    rewrite_stmts(finally, array_id, aliases, fields);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                rewrite_expr(discriminant, array_id, aliases, fields);
                for case in cases {
                    if let Some(test) = &mut case.test {
                        rewrite_expr(test, array_id, aliases, fields);
                    }
                    rewrite_stmts(&mut case.body, array_id, aliases, fields);
                }
            }
            Stmt::Labeled { .. }
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
        }
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::CompareOp;

    fn object(value: i64) -> Expr {
        Expr::Object(vec![("component".to_string(), Expr::Integer(value))])
    }

    fn property(object: Expr, name: &str) -> Expr {
        Expr::PropertyGet {
            object: Box::new(object),
            property: name.to_string(),
            byte_offset: 0,
        }
    }

    fn aggregate_fixture(observe_identity: bool) -> Module {
        let mut module = Module::new("aggregate-scalar.ts");
        module.init = vec![
            Stmt::Let {
                id: 1,
                name: "values".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Array(vec![object(10), object(20)])),
            },
            Stmt::Let {
                id: 2,
                name: "first".to_string(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::Integer(0)),
                }),
            },
            Stmt::Expr(if observe_identity {
                Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(2)),
                    right: Box::new(Expr::LocalGet(2)),
                }
            } else {
                property(Expr::LocalGet(2), "component")
            }),
        ];
        module
    }

    #[test]
    fn replaces_nested_carriers_with_scalar_field_locals() {
        let mut module = aggregate_fixture(false);
        run(&mut module);

        assert!(module.init.iter().all(|stmt| {
            !matches!(
                stmt,
                Stmt::Let {
                    init: Some(Expr::Array(_) | Expr::Object(_)),
                    ..
                }
            )
        }));
        assert!(!module
            .init
            .iter()
            .any(|stmt| matches!(stmt, Stmt::Let { id: 2, .. })));
        assert!(matches!(
            module.init.last(),
            Some(Stmt::Expr(Expr::LocalGet(_)))
        ));
    }

    #[test]
    fn identity_observation_keeps_materialized_aggregate() {
        let mut module = aggregate_fixture(true);
        run(&mut module);

        assert!(module.init.iter().any(|stmt| {
            matches!(
                stmt,
                Stmt::Let {
                    id: 1,
                    init: Some(Expr::Array(_)),
                    ..
                }
            )
        }));
        assert!(module
            .init
            .iter()
            .any(|stmt| matches!(stmt, Stmt::Let { id: 2, .. })));
    }

    #[test]
    fn mutation_reflection_and_unknown_calls_keep_materialized_aggregate() {
        let hazards = vec![
            Expr::PropertySet {
                object: Box::new(Expr::LocalGet(2)),
                property: "component".to_string(),
                value: Box::new(Expr::Integer(30)),
            },
            Expr::ObjectKeys(Box::new(Expr::LocalGet(2))),
            Expr::Call {
                callee: Box::new(Expr::FuncRef(99)),
                args: vec![Expr::LocalGet(2)],
                type_args: Vec::new(),
                byte_offset: 0,
            },
        ];

        for hazard in hazards {
            let mut module = aggregate_fixture(false);
            *module.init.last_mut().expect("observer statement") = Stmt::Expr(hazard);
            run(&mut module);

            assert!(module.init.iter().any(|stmt| {
                matches!(
                    stmt,
                    Stmt::Let {
                        id: 1,
                        init: Some(Expr::Array(_)),
                        ..
                    }
                )
            }));
        }
    }
}
