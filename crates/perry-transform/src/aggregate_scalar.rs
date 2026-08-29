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
use perry_hir::{Expr, Function, Module, Stmt};

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

    // LocalIds are module-unique, but generated object/class methods live in
    // separate HIR function bodies. A carrier declared in one body must stay
    // materialized when another body references it, even if every use in the
    // declaring body looks scalarizable. Count the distinct HIR regions that
    // reference each local before mutating any of them.
    let mut region_refs = vec![collect_stmt_refs(&module.init)];
    region_refs.extend(module.functions.iter().map(collect_function_refs));
    for class in &module.classes {
        if let Some(constructor) = &class.constructor {
            region_refs.push(collect_function_refs(constructor));
        }
        region_refs.extend(class.methods.iter().map(collect_function_refs));
        region_refs.extend(
            class
                .getters
                .iter()
                .map(|(_, function)| collect_function_refs(function)),
        );
        region_refs.extend(
            class
                .setters
                .iter()
                .map(|(_, function)| collect_function_refs(function)),
        );
        region_refs.extend(class.static_methods.iter().map(collect_function_refs));
    }
    // Computed members are code-generated from their own retained Function
    // bodies rather than passed through scalarize_stmts below. They still
    // count as separate consumers of a carrier declared elsewhere.
    for class in &module.classes {
        region_refs.extend(
            class
                .computed_members
                .iter()
                .map(|member| collect_function_refs(&member.function)),
        );
    }
    let mut reference_region_counts: HashMap<LocalId, usize> = HashMap::new();
    for refs in &region_refs {
        for id in refs {
            *reference_region_counts.entry(*id).or_default() += 1;
        }
    }
    let mut region_index = 0;

    scalarize_stmts(
        &mut module.init,
        &mut next_local_id,
        &mut source_span_remaps,
        &anon_shape_fields,
        &region_refs[region_index],
        &reference_region_counts,
    );
    region_index += 1;
    for function in &mut module.functions {
        scalarize_stmts(
            &mut function.body,
            &mut next_local_id,
            &mut source_span_remaps,
            &anon_shape_fields,
            &region_refs[region_index],
            &reference_region_counts,
        );
        region_index += 1;
    }
    for class in &mut module.classes {
        if let Some(constructor) = &mut class.constructor {
            scalarize_stmts(
                &mut constructor.body,
                &mut next_local_id,
                &mut source_span_remaps,
                &anon_shape_fields,
                &region_refs[region_index],
                &reference_region_counts,
            );
            region_index += 1;
        }
        for method in &mut class.methods {
            scalarize_stmts(
                &mut method.body,
                &mut next_local_id,
                &mut source_span_remaps,
                &anon_shape_fields,
                &region_refs[region_index],
                &reference_region_counts,
            );
            region_index += 1;
        }
        for (_, getter) in &mut class.getters {
            scalarize_stmts(
                &mut getter.body,
                &mut next_local_id,
                &mut source_span_remaps,
                &anon_shape_fields,
                &region_refs[region_index],
                &reference_region_counts,
            );
            region_index += 1;
        }
        for (_, setter) in &mut class.setters {
            scalarize_stmts(
                &mut setter.body,
                &mut next_local_id,
                &mut source_span_remaps,
                &anon_shape_fields,
                &region_refs[region_index],
                &reference_region_counts,
            );
            region_index += 1;
        }
        for method in &mut class.static_methods {
            scalarize_stmts(
                &mut method.body,
                &mut next_local_id,
                &mut source_span_remaps,
                &anon_shape_fields,
                &region_refs[region_index],
                &reference_region_counts,
            );
            region_index += 1;
        }
    }

    for (source_id, new_id) in source_span_remaps {
        if let Some(span) = module.local_source_spans.get(&source_id).copied() {
            module.local_source_spans.insert(new_id, span);
        }
    }
}

fn collect_stmt_refs(stmts: &[Stmt]) -> HashSet<LocalId> {
    let mut refs = Vec::new();
    let mut visited = HashSet::new();
    for stmt in stmts {
        perry_hir::collect_local_refs_stmt(stmt, &mut refs, &mut visited);
    }
    refs.into_iter().collect()
}

fn collect_function_refs(function: &Function) -> HashSet<LocalId> {
    let mut refs = collect_stmt_refs(&function.body);
    refs.extend(function.captures.iter().copied());
    refs
}

fn scalarize_stmts(
    stmts: &mut Vec<Stmt>,
    next_local_id: &mut LocalId,
    source_span_remaps: &mut Vec<(LocalId, LocalId)>,
    anon_shape_fields: &AnonShapeFields,
    region_refs: &HashSet<LocalId>,
    reference_region_counts: &HashMap<LocalId, usize>,
) {
    // Early-return inlining represents a returned value as
    //
    //   let result = undefined;
    //   do { ...; result = new __AnonShape(...); break; } while (false);
    //
    // The ordinary codegen scalar-replacement collector only sees a `New`
    // directly in a Let initializer, so this canonical merge used to force a
    // heap record even when every consumer merely read known fields. Promote
    // those merge records to one mutable scalar local per field first.
    let return_record_candidates: Vec<LocalId> = stmts
        .windows(2)
        .filter_map(|pair| match (&pair[0], &pair[1]) {
            (
                Stmt::Let {
                    id,
                    mutable: true,
                    init: Some(Expr::Undefined),
                    ..
                },
                Stmt::DoWhile {
                    condition: Expr::Bool(false),
                    ..
                },
            ) => Some(*id),
            _ => None,
        })
        .collect();
    for record_id in return_record_candidates {
        let _ = scalarize_return_record_candidate(
            stmts,
            record_id,
            next_local_id,
            source_span_remaps,
            anon_shape_fields,
            region_refs,
            reference_region_counts,
        );
    }

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
            region_refs,
            reference_region_counts,
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
                    region_refs,
                    reference_region_counts,
                );
                if let Some(else_branch) = else_branch {
                    scalarize_stmts(
                        else_branch,
                        next_local_id,
                        source_span_remaps,
                        anon_shape_fields,
                        region_refs,
                        reference_region_counts,
                    );
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                scalarize_stmts(
                    body,
                    next_local_id,
                    source_span_remaps,
                    anon_shape_fields,
                    region_refs,
                    reference_region_counts,
                );
            }
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    let mut init_vec = vec![(**init).clone()];
                    scalarize_stmts(
                        &mut init_vec,
                        next_local_id,
                        source_span_remaps,
                        anon_shape_fields,
                        region_refs,
                        reference_region_counts,
                    );
                    if init_vec.len() == 1 {
                        **init = init_vec.remove(0);
                    }
                }
                scalarize_stmts(
                    body,
                    next_local_id,
                    source_span_remaps,
                    anon_shape_fields,
                    region_refs,
                    reference_region_counts,
                );
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                scalarize_stmts(
                    body,
                    next_local_id,
                    source_span_remaps,
                    anon_shape_fields,
                    region_refs,
                    reference_region_counts,
                );
                if let Some(catch) = catch {
                    scalarize_stmts(
                        &mut catch.body,
                        next_local_id,
                        source_span_remaps,
                        anon_shape_fields,
                        region_refs,
                        reference_region_counts,
                    );
                }
                if let Some(finally) = finally {
                    scalarize_stmts(
                        finally,
                        next_local_id,
                        source_span_remaps,
                        anon_shape_fields,
                        region_refs,
                        reference_region_counts,
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
                        region_refs,
                        reference_region_counts,
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

fn scalarize_return_record_candidate(
    stmts: &mut Vec<Stmt>,
    record_id: LocalId,
    next_local_id: &mut LocalId,
    source_span_remaps: &mut Vec<(LocalId, LocalId)>,
    anon_shape_fields: &AnonShapeFields,
    region_refs: &HashSet<LocalId>,
    reference_region_counts: &HashMap<LocalId, usize>,
) -> bool {
    let own_region_reference = usize::from(region_refs.contains(&record_id));
    if reference_region_counts
        .get(&record_id)
        .copied()
        .unwrap_or_default()
        > own_region_reference
    {
        return false;
    }
    let Some(declaration_index) = stmts.iter().position(|stmt| {
        matches!(
            stmt,
            Stmt::Let {
                id,
                mutable: true,
                init: Some(Expr::Undefined),
                ..
            } if *id == record_id
        )
    }) else {
        return false;
    };
    let Some(Stmt::DoWhile {
        body: merge_body,
        condition: Expr::Bool(false),
    }) = stmts.get(declaration_index + 1)
    else {
        return false;
    };

    let mut assigned_shapes = Vec::new();
    collect_return_record_assignments(merge_body, record_id, &mut assigned_shapes);
    if assigned_shapes.is_empty() {
        return false;
    }
    let mut field_order = Vec::new();
    let mut admitted_shapes: HashMap<String, Vec<String>> = HashMap::new();
    for class_name in assigned_shapes {
        let Some(fields) = anon_shape_fields.get(&class_name) else {
            return false;
        };
        if fields.is_empty() {
            return false;
        }
        for field in fields {
            if !field_order.contains(field) {
                field_order.push(field.clone());
            }
        }
        admitted_shapes.insert(class_name, fields.clone());
    }
    if field_order.is_empty() || field_order.len() > MAX_SCALAR_AGGREGATE_FIELDS {
        return false;
    }

    // Every exit from the synthetic do/while that reaches subsequent field
    // reads must first write a record. `return undefined` becomes a bare
    // Break; rejecting such a break preserves the original TypeError behavior
    // instead of silently turning it into an all-undefined record.
    if !merge_body_ends_with_record_assignment(merge_body, record_id, &admitted_shapes) {
        return false;
    }

    if !return_record_stmts_are_safe(
        merge_body,
        record_id,
        &admitted_shapes,
        &field_order,
        true,
        false,
    ) {
        return false;
    }
    for (index, stmt) in stmts.iter().enumerate() {
        if index == declaration_index || index == declaration_index + 1 {
            continue;
        }
        if !return_record_stmts_are_safe(
            std::slice::from_ref(stmt),
            record_id,
            &admitted_shapes,
            &field_order,
            false,
            true,
        ) {
            return false;
        }
    }

    let mut field_locals = HashMap::new();
    let mut replacement_declarations = Vec::with_capacity(field_order.len());
    for (index, field) in field_order.iter().enumerate() {
        let id = *next_local_id;
        *next_local_id = next_local_id.saturating_add(1);
        source_span_remaps.push((record_id, id));
        field_locals.insert(field.clone(), id);
        replacement_declarations.push(Stmt::Let {
            id,
            name: format!("__perry_return_record_{record_id}_{index}"),
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Undefined),
        });
    }

    rewrite_return_record_stmts(
        stmts,
        record_id,
        &admitted_shapes,
        &field_order,
        &field_locals,
    );
    let Some(declaration_index) = stmts
        .iter()
        .position(|stmt| matches!(stmt, Stmt::Let { id, .. } if *id == record_id))
    else {
        return false;
    };
    stmts.splice(
        declaration_index..=declaration_index,
        replacement_declarations,
    );
    true
}

fn collect_return_record_assignments(
    stmts: &[Stmt],
    record_id: LocalId,
    assigned_shapes: &mut Vec<String>,
) {
    for stmt in stmts {
        if let Stmt::Expr(Expr::LocalSet(id, value)) = stmt {
            if *id == record_id {
                if let Expr::New { class_name, .. } = value.as_ref() {
                    assigned_shapes.push(class_name.clone());
                }
            }
        }
        match stmt {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_return_record_assignments(then_branch, record_id, assigned_shapes);
                if let Some(else_branch) = else_branch {
                    collect_return_record_assignments(else_branch, record_id, assigned_shapes);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                collect_return_record_assignments(body, record_id, assigned_shapes)
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_return_record_assignments(body, record_id, assigned_shapes);
                if let Some(catch) = catch {
                    collect_return_record_assignments(&catch.body, record_id, assigned_shapes);
                }
                if let Some(finally) = finally {
                    collect_return_record_assignments(finally, record_id, assigned_shapes);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_return_record_assignments(&case.body, record_id, assigned_shapes);
                }
            }
            Stmt::Labeled { body, .. } => collect_return_record_assignments(
                std::slice::from_ref(body.as_ref()),
                record_id,
                assigned_shapes,
            ),
            _ => {}
        }
    }
}

fn is_return_record_assignment(
    stmt: &Stmt,
    record_id: LocalId,
    admitted_shapes: &HashMap<String, Vec<String>>,
) -> bool {
    matches!(
        stmt,
        Stmt::Expr(Expr::LocalSet(id, value))
            if *id == record_id
                && matches!(value.as_ref(), Expr::New { class_name, .. } if admitted_shapes.contains_key(class_name))
    )
}

fn merge_body_ends_with_record_assignment(
    stmts: &[Stmt],
    record_id: LocalId,
    admitted_shapes: &HashMap<String, Vec<String>>,
) -> bool {
    // The wrapper's fallthrough must also be a converted return.
    if !matches!(stmts.last(), Some(Stmt::Break))
        || stmts.len() < 2
        || !is_return_record_assignment(&stmts[stmts.len() - 2], record_id, admitted_shapes)
    {
        return false;
    }

    for (index, stmt) in stmts.iter().enumerate() {
        match stmt {
            Stmt::Break => {
                if index == 0
                    || !is_return_record_assignment(&stmts[index - 1], record_id, admitted_shapes)
                {
                    return false;
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                if !merge_nested_breaks_follow_assignment(then_branch, record_id, admitted_shapes)
                    || else_branch.as_ref().is_some_and(|branch| {
                        !merge_nested_breaks_follow_assignment(branch, record_id, admitted_shapes)
                    })
                {
                    return false;
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                if !merge_nested_breaks_follow_assignment(body, record_id, admitted_shapes)
                    || catch.as_ref().is_some_and(|catch| {
                        !merge_nested_breaks_follow_assignment(
                            &catch.body,
                            record_id,
                            admitted_shapes,
                        )
                    })
                    || finally.as_ref().is_some_and(|finally| {
                        !merge_nested_breaks_follow_assignment(finally, record_id, admitted_shapes)
                    })
                {
                    return false;
                }
            }
            Stmt::Switch { cases, .. } => {
                if cases.iter().any(|case| {
                    !merge_nested_breaks_follow_assignment(&case.body, record_id, admitted_shapes)
                }) {
                    return false;
                }
            }
            // Breaks in a nested loop target that loop rather than this
            // synthetic wrapper and are deliberately not inspected here.
            _ => {}
        }
    }
    true
}

fn merge_nested_breaks_follow_assignment(
    stmts: &[Stmt],
    record_id: LocalId,
    admitted_shapes: &HashMap<String, Vec<String>>,
) -> bool {
    for (index, stmt) in stmts.iter().enumerate() {
        match stmt {
            Stmt::Break => {
                if index == 0
                    || !is_return_record_assignment(&stmts[index - 1], record_id, admitted_shapes)
                {
                    return false;
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                if !merge_nested_breaks_follow_assignment(then_branch, record_id, admitted_shapes)
                    || else_branch.as_ref().is_some_and(|branch| {
                        !merge_nested_breaks_follow_assignment(branch, record_id, admitted_shapes)
                    })
                {
                    return false;
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                if !merge_nested_breaks_follow_assignment(body, record_id, admitted_shapes)
                    || catch.as_ref().is_some_and(|catch| {
                        !merge_nested_breaks_follow_assignment(
                            &catch.body,
                            record_id,
                            admitted_shapes,
                        )
                    })
                    || finally.as_ref().is_some_and(|finally| {
                        !merge_nested_breaks_follow_assignment(finally, record_id, admitted_shapes)
                    })
                {
                    return false;
                }
            }
            Stmt::Switch { cases, .. } => {
                if cases.iter().any(|case| {
                    !merge_nested_breaks_follow_assignment(&case.body, record_id, admitted_shapes)
                }) {
                    return false;
                }
            }
            Stmt::While { .. } | Stmt::DoWhile { .. } | Stmt::For { .. } => {}
            _ => {}
        }
    }
    true
}

fn return_record_stmts_are_safe(
    stmts: &[Stmt],
    record_id: LocalId,
    admitted_shapes: &HashMap<String, Vec<String>>,
    field_order: &[String],
    allow_assignments: bool,
    allow_reads: bool,
) -> bool {
    fn expr_is_safe(
        expr: &Expr,
        record_id: LocalId,
        field_order: &[String],
        allow_reads: bool,
    ) -> bool {
        if let Expr::PropertyGet {
            object, property, ..
        } = expr
        {
            if matches!(object.as_ref(), Expr::LocalGet(id) if *id == record_id) {
                return allow_reads && field_order.contains(property);
            }
        }
        if matches!(expr, Expr::LocalGet(id) if *id == record_id)
            || matches!(expr, Expr::LocalSet(id, _) if *id == record_id)
            || matches!(expr, Expr::Update { id, .. } if *id == record_id)
        {
            return false;
        }
        if let Expr::Closure { body, .. } = expr {
            return return_record_stmts_are_safe(
                body,
                record_id,
                &HashMap::new(),
                field_order,
                false,
                false,
            );
        }
        let mut safe = true;
        perry_hir::walker::walk_expr_children(expr, &mut |child| {
            if !expr_is_safe(child, record_id, field_order, allow_reads) {
                safe = false;
            }
        });
        safe
    }

    for stmt in stmts {
        if allow_assignments && is_return_record_assignment(stmt, record_id, admitted_shapes) {
            let Stmt::Expr(Expr::LocalSet(_, value)) = stmt else {
                unreachable!();
            };
            let Expr::New { args, .. } = value.as_ref() else {
                unreachable!();
            };
            if args
                .iter()
                .any(|arg| !expr_is_safe(arg, record_id, field_order, false))
            {
                return false;
            }
            continue;
        }
        let expressions_safe = match stmt {
            Stmt::Let { init, .. } => init
                .as_ref()
                .is_none_or(|expr| expr_is_safe(expr, record_id, field_order, allow_reads)),
            Stmt::Expr(expr) | Stmt::Throw(expr) | Stmt::Return(Some(expr)) => {
                expr_is_safe(expr, record_id, field_order, allow_reads)
            }
            Stmt::If { condition, .. }
            | Stmt::While { condition, .. }
            | Stmt::DoWhile { condition, .. } => {
                expr_is_safe(condition, record_id, field_order, allow_reads)
            }
            Stmt::For {
                init,
                condition,
                update,
                ..
            } => {
                init.as_deref().is_none_or(|init| {
                    return_record_stmts_are_safe(
                        std::slice::from_ref(init),
                        record_id,
                        admitted_shapes,
                        field_order,
                        allow_assignments,
                        allow_reads,
                    )
                }) && condition
                    .as_ref()
                    .is_none_or(|expr| expr_is_safe(expr, record_id, field_order, allow_reads))
                    && update
                        .as_ref()
                        .is_none_or(|expr| expr_is_safe(expr, record_id, field_order, allow_reads))
            }
            Stmt::Switch { discriminant, .. } => {
                expr_is_safe(discriminant, record_id, field_order, allow_reads)
            }
            Stmt::PreallocateBoxes(ids)
            | Stmt::PreallocateTdzBoxes(ids)
            | Stmt::ReleaseBoxes(ids) => !ids.contains(&record_id),
            _ => true,
        };
        if !expressions_safe {
            return false;
        }
        match stmt {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                if !return_record_stmts_are_safe(
                    then_branch,
                    record_id,
                    admitted_shapes,
                    field_order,
                    allow_assignments,
                    allow_reads,
                ) || else_branch.as_ref().is_some_and(|branch| {
                    !return_record_stmts_are_safe(
                        branch,
                        record_id,
                        admitted_shapes,
                        field_order,
                        allow_assignments,
                        allow_reads,
                    )
                }) {
                    return false;
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                if !return_record_stmts_are_safe(
                    body,
                    record_id,
                    admitted_shapes,
                    field_order,
                    allow_assignments,
                    allow_reads,
                ) {
                    return false;
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                if !return_record_stmts_are_safe(
                    body,
                    record_id,
                    admitted_shapes,
                    field_order,
                    allow_assignments,
                    allow_reads,
                ) || catch.as_ref().is_some_and(|catch| {
                    !return_record_stmts_are_safe(
                        &catch.body,
                        record_id,
                        admitted_shapes,
                        field_order,
                        allow_assignments,
                        allow_reads,
                    )
                }) || finally.as_ref().is_some_and(|finally| {
                    !return_record_stmts_are_safe(
                        finally,
                        record_id,
                        admitted_shapes,
                        field_order,
                        allow_assignments,
                        allow_reads,
                    )
                }) {
                    return false;
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    if case.test.as_ref().is_some_and(|test| {
                        !expr_is_safe(test, record_id, field_order, allow_reads)
                    }) || !return_record_stmts_are_safe(
                        &case.body,
                        record_id,
                        admitted_shapes,
                        field_order,
                        allow_assignments,
                        allow_reads,
                    ) {
                        return false;
                    }
                }
            }
            Stmt::Labeled { body, .. } => {
                if !return_record_stmts_are_safe(
                    std::slice::from_ref(body.as_ref()),
                    record_id,
                    admitted_shapes,
                    field_order,
                    allow_assignments,
                    allow_reads,
                ) {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

fn rewrite_return_record_stmts(
    stmts: &mut Vec<Stmt>,
    record_id: LocalId,
    admitted_shapes: &HashMap<String, Vec<String>>,
    field_order: &[String],
    field_locals: &HashMap<String, LocalId>,
) {
    let mut index = 0;
    while index < stmts.len() {
        if is_return_record_assignment(&stmts[index], record_id, admitted_shapes) {
            let Stmt::Expr(Expr::LocalSet(_, value)) = &stmts[index] else {
                unreachable!();
            };
            let Expr::New {
                class_name, args, ..
            } = value.as_ref()
            else {
                unreachable!();
            };
            let shape_fields = admitted_shapes
                .get(class_name)
                .expect("assignment shape was admitted");
            let mut replacement = Vec::new();
            for (argument_index, argument) in args.iter().enumerate() {
                if let Some(field) = shape_fields.get(argument_index) {
                    replacement.push(Stmt::Expr(Expr::LocalSet(
                        *field_locals.get(field).expect("field local exists"),
                        Box::new(argument.clone()),
                    )));
                } else {
                    replacement.push(Stmt::Expr(argument.clone()));
                }
            }
            for field in field_order {
                if !shape_fields.contains(field) {
                    replacement.push(Stmt::Expr(Expr::LocalSet(
                        *field_locals.get(field).expect("field local exists"),
                        Box::new(Expr::Undefined),
                    )));
                }
            }
            let replacement_len = replacement.len();
            stmts.splice(index..=index, replacement);
            index += replacement_len;
            continue;
        }

        match &mut stmts[index] {
            Stmt::Let { init, .. } => {
                if let Some(expr) = init {
                    rewrite_return_record_expr(expr, record_id, field_locals);
                }
            }
            Stmt::Expr(expr) | Stmt::Throw(expr) | Stmt::Return(Some(expr)) => {
                rewrite_return_record_expr(expr, record_id, field_locals)
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                rewrite_return_record_expr(condition, record_id, field_locals);
                rewrite_return_record_stmts(
                    then_branch,
                    record_id,
                    admitted_shapes,
                    field_order,
                    field_locals,
                );
                if let Some(else_branch) = else_branch {
                    rewrite_return_record_stmts(
                        else_branch,
                        record_id,
                        admitted_shapes,
                        field_order,
                        field_locals,
                    );
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                rewrite_return_record_expr(condition, record_id, field_locals);
                rewrite_return_record_stmts(
                    body,
                    record_id,
                    admitted_shapes,
                    field_order,
                    field_locals,
                );
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    let mut init_stmts = vec![*init.clone()];
                    rewrite_return_record_stmts(
                        &mut init_stmts,
                        record_id,
                        admitted_shapes,
                        field_order,
                        field_locals,
                    );
                    if init_stmts.len() == 1 {
                        **init = init_stmts.remove(0);
                    }
                }
                if let Some(condition) = condition {
                    rewrite_return_record_expr(condition, record_id, field_locals);
                }
                if let Some(update) = update {
                    rewrite_return_record_expr(update, record_id, field_locals);
                }
                rewrite_return_record_stmts(
                    body,
                    record_id,
                    admitted_shapes,
                    field_order,
                    field_locals,
                );
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                rewrite_return_record_stmts(
                    body,
                    record_id,
                    admitted_shapes,
                    field_order,
                    field_locals,
                );
                if let Some(catch) = catch {
                    rewrite_return_record_stmts(
                        &mut catch.body,
                        record_id,
                        admitted_shapes,
                        field_order,
                        field_locals,
                    );
                }
                if let Some(finally) = finally {
                    rewrite_return_record_stmts(
                        finally,
                        record_id,
                        admitted_shapes,
                        field_order,
                        field_locals,
                    );
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                rewrite_return_record_expr(discriminant, record_id, field_locals);
                for case in cases {
                    if let Some(test) = &mut case.test {
                        rewrite_return_record_expr(test, record_id, field_locals);
                    }
                    rewrite_return_record_stmts(
                        &mut case.body,
                        record_id,
                        admitted_shapes,
                        field_order,
                        field_locals,
                    );
                }
            }
            Stmt::Labeled { body, .. } => {
                let mut body_stmts = vec![*body.clone()];
                rewrite_return_record_stmts(
                    &mut body_stmts,
                    record_id,
                    admitted_shapes,
                    field_order,
                    field_locals,
                );
                if body_stmts.len() == 1 {
                    **body = body_stmts.remove(0);
                }
            }
            _ => {}
        }
        index += 1;
    }
}

fn rewrite_return_record_expr(
    expr: &mut Expr,
    record_id: LocalId,
    field_locals: &HashMap<String, LocalId>,
) {
    if let Expr::PropertyGet {
        object, property, ..
    } = expr
    {
        if matches!(object.as_ref(), Expr::LocalGet(id) if *id == record_id) {
            if let Some(field_id) = field_locals.get(property) {
                *expr = Expr::LocalGet(*field_id);
                return;
            }
        }
    }
    if let Expr::Closure { body, .. } = expr {
        // Safety analysis permits closure reads only when they remain within
        // the same HIR region; rewrite their bodies explicitly because the
        // generic expression walker does not descend into closures.
        rewrite_return_record_stmts(body, record_id, &HashMap::new(), &[], field_locals);
    }
    perry_hir::walker::walk_expr_children_mut(expr, &mut |child| {
        rewrite_return_record_expr(child, record_id, field_locals)
    });
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
    region_refs: &HashSet<LocalId>,
    reference_region_counts: &HashMap<LocalId, usize>,
) -> bool {
    let own_region_reference = usize::from(region_refs.contains(&array_id));
    if reference_region_counts
        .get(&array_id)
        .copied()
        .unwrap_or_default()
        > own_region_reference
    {
        return false;
    }
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
            body,
            captures,
            mutable_captures,
            ..
        } => {
            // Module-scope locals are intentionally omitted from a closure's
            // capture lists, but the closure body still refers to their
            // LocalIds directly. The generic expression walker does not enter
            // closure statement bodies, so inspect them explicitly before
            // deleting a scalarized carrier (#9048).
            let body_refs = collect_stmt_refs(body);
            if captures
                .iter()
                .chain(mutable_captures.iter())
                .chain(body_refs.iter())
                .any(|id| *id == array_id || aliases.contains_key(id))
            {
                return false;
            }
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

    #[test]
    fn reference_from_generated_function_keeps_materialized_aggregate() {
        let mut module = aggregate_fixture(false);
        module.functions.push(Function {
            id: 99,
            name: "__obj_method_computed".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![Stmt::Return(Some(Expr::LocalGet(1)))],
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        });

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

    #[test]
    fn module_local_reference_from_closure_body_keeps_materialized_aggregate() {
        let mut module = aggregate_fixture(false);
        *module.init.last_mut().expect("observer statement") = Stmt::Expr(Expr::Closure {
            func_id: 99,
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![Stmt::Return(Some(property(
                Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(1)),
                    index: Box::new(Expr::Integer(0)),
                },
                "component",
            )))],
            captures: Vec::new(),
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: true,
        });

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

    fn shape_new(name: &str, args: Vec<Expr>) -> Expr {
        Expr::New {
            class_name: name.to_string(),
            args,
            type_args: Vec::new(),
            byte_offset: 0,
            cap_args_appended: 0,
        }
    }

    fn returned_record_fixture(with_undefined_exit: bool, observe_identity: bool) -> Vec<Stmt> {
        let early_exit = if with_undefined_exit {
            vec![Stmt::Break]
        } else {
            vec![
                Stmt::Expr(Expr::LocalSet(
                    1,
                    Box::new(shape_new("__AnonShape_short", vec![Expr::Integer(1)])),
                )),
                Stmt::Break,
            ]
        };
        vec![
            Stmt::Let {
                id: 1,
                name: "result".to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Undefined),
            },
            Stmt::DoWhile {
                body: vec![
                    Stmt::If {
                        condition: Expr::Bool(false),
                        then_branch: early_exit,
                        else_branch: None,
                    },
                    Stmt::Expr(Expr::LocalSet(
                        1,
                        Box::new(shape_new(
                            "__AnonShape_long",
                            vec![Expr::Integer(2), Expr::Integer(7)],
                        )),
                    )),
                    Stmt::Break,
                ],
                condition: Expr::Bool(false),
            },
            Stmt::Expr(if observe_identity {
                Expr::Compare {
                    op: CompareOp::Eq,
                    left: Box::new(Expr::LocalGet(1)),
                    right: Box::new(Expr::LocalGet(1)),
                }
            } else {
                property(Expr::LocalGet(1), "detail")
            }),
        ]
    }

    fn scalarize_return_fixture(stmts: &mut Vec<Stmt>) -> bool {
        let shapes = HashMap::from([
            ("__AnonShape_short".to_string(), vec!["type".to_string()]),
            (
                "__AnonShape_long".to_string(),
                vec!["type".to_string(), "detail".to_string()],
            ),
        ]);
        let mut next_local_id = 100;
        let mut source_span_remaps = Vec::new();
        scalarize_return_record_candidate(
            stmts,
            1,
            &mut next_local_id,
            &mut source_span_remaps,
            &shapes,
            &HashSet::from([1]),
            &HashMap::from([(1, 1)]),
        )
    }

    #[test]
    fn scalarizes_multi_shape_inlined_return_record() {
        let mut stmts = returned_record_fixture(false, false);
        assert!(scalarize_return_fixture(&mut stmts));

        assert!(!stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::Let { id: 1, .. })));
        assert!(stmts.iter().any(|stmt| {
            matches!(stmt, Stmt::Let { name, .. } if name == "__perry_return_record_1_0")
        }));
        assert!(matches!(
            stmts.last(),
            Some(Stmt::Expr(Expr::LocalGet(101)))
        ));
        let debug = format!("{stmts:?}");
        assert!(!debug.contains("__AnonShape_"));
        assert!(debug.contains("LocalSet(101, Undefined)"));
    }

    #[test]
    fn undefined_exit_keeps_inlined_return_record_materialized() {
        let mut stmts = returned_record_fixture(true, false);
        assert!(!scalarize_return_fixture(&mut stmts));
        assert!(format!("{stmts:?}").contains("__AnonShape_long"));
    }

    #[test]
    fn identity_observation_keeps_inlined_return_record_materialized() {
        let mut stmts = returned_record_fixture(false, true);
        assert!(!scalarize_return_fixture(&mut stmts));
        assert!(format!("{stmts:?}").contains("__AnonShape_short"));
    }
}
