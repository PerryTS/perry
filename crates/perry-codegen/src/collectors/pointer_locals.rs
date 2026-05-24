use perry_hir::{BinaryOp, Expr};
use perry_types::Type;
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
enum LocalWrite {
    Expr(Expr),
    NonPointer,
}

pub fn collect_pointer_typed_locals(
    params: &[perry_hir::Param],
    stmts: &[perry_hir::Stmt],
    flat_const_ids: &HashSet<u32>,
) -> std::collections::HashMap<u32, u32> {
    use perry_hir::Stmt;
    fn is_ptr_typed(ty: &Type) -> bool {
        matches!(
            ty,
            Type::String
                | Type::Array(_)
                | Type::Tuple(_)
                | Type::Object(_)
                | Type::Named(_)
                | Type::Promise(_)
                | Type::Function(_)
                | Type::BigInt
                | Type::Any
                | Type::Unknown
        ) || matches!(ty, Type::Union(variants) if variants.iter().any(is_ptr_typed))
    }

    fn is_definitely_non_pointer_type(ty: &Type) -> bool {
        matches!(
            ty,
            Type::Number
                | Type::Int32
                | Type::Boolean
                | Type::Null
                | Type::Void
                | Type::Never
                | Type::Symbol
        ) || matches!(ty, Type::Union(variants) if variants.iter().all(is_definitely_non_pointer_type))
    }

    fn expr_value_type(
        expr: &Expr,
        local_types: &HashMap<u32, Type>,
        local_value_types: &HashMap<u32, Type>,
        non_pointer_locals: &HashSet<u32>,
    ) -> Option<Type> {
        match expr {
            Expr::Undefined => Some(Type::Void),
            Expr::Null => Some(Type::Null),
            Expr::Bool(_) | Expr::Compare { .. } => Some(Type::Boolean),
            Expr::Number(_)
            | Expr::Integer(_)
            | Expr::Uint8ArrayLength(_)
            | Expr::Uint8ArrayGet { .. }
            | Expr::BufferLength(_)
            | Expr::BufferIndexGet { .. }
            | Expr::MathFloor(_)
            | Expr::MathCeil(_)
            | Expr::MathRound(_)
            | Expr::MathAbs(_)
            | Expr::MathSqrt(_)
            | Expr::MathLog(_)
            | Expr::MathLog2(_)
            | Expr::MathLog10(_)
            | Expr::MathPow(_, _)
            | Expr::MathMin(_)
            | Expr::MathMax(_)
            | Expr::MathImul(_, _)
            | Expr::MathRandom
            | Expr::MathSin(_)
            | Expr::MathCos(_)
            | Expr::MathTan(_)
            | Expr::MathAsin(_)
            | Expr::MathAcos(_)
            | Expr::MathAtan(_)
            | Expr::MathAtan2(_, _)
            | Expr::MathCbrt(_)
            | Expr::MathHypot(_)
            | Expr::MathFround(_)
            | Expr::MathClz32(_)
            | Expr::MathExpm1(_)
            | Expr::MathLog1p(_)
            | Expr::MathSinh(_)
            | Expr::MathCosh(_)
            | Expr::MathTanh(_)
            | Expr::MathAsinh(_)
            | Expr::MathAcosh(_)
            | Expr::MathAtanh(_)
            | Expr::MathExp(_)
            | Expr::PerformanceNow => Some(Type::Number),
            Expr::String(_)
            | Expr::WtfString(_)
            | Expr::I18nString { .. }
            | Expr::TypeOf(_)
            | Expr::JsonStringify(_)
            | Expr::JsonStringifyPretty { .. }
            | Expr::JsonStringifyFull(..) => Some(Type::String),
            Expr::LocalGet(id) => local_value_types
                .get(id)
                .or_else(|| local_types.get(id))
                .cloned()
                .or_else(|| {
                    if non_pointer_locals.contains(id) {
                        Some(Type::Number)
                    } else {
                        None
                    }
                }),
            Expr::Unary { op, .. } => Some(match op {
                perry_hir::UnaryOp::Not => Type::Boolean,
                perry_hir::UnaryOp::Neg | perry_hir::UnaryOp::Pos | perry_hir::UnaryOp::BitNot => {
                    Type::Number
                }
            }),
            Expr::Binary { op, left, right } => {
                if matches!(op, BinaryOp::Add) {
                    if expr_is_known_non_pointer(
                        left,
                        local_types,
                        local_value_types,
                        non_pointer_locals,
                    ) && expr_is_known_non_pointer(
                        right,
                        local_types,
                        local_value_types,
                        non_pointer_locals,
                    ) {
                        Some(Type::Number)
                    } else {
                        None
                    }
                } else {
                    Some(Type::Number)
                }
            }
            Expr::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                let then_ty = expr_value_type(
                    then_expr,
                    local_types,
                    local_value_types,
                    non_pointer_locals,
                )?;
                let else_ty = expr_value_type(
                    else_expr,
                    local_types,
                    local_value_types,
                    non_pointer_locals,
                )?;
                if then_ty == else_ty {
                    Some(then_ty)
                } else {
                    None
                }
            }
            Expr::Sequence(exprs) => exprs.last().and_then(|last| {
                expr_value_type(last, local_types, local_value_types, non_pointer_locals)
            }),
            Expr::Array(elements) => {
                let mut elem_ty: Option<Type> = None;
                for elem in elements {
                    let Some(ty) =
                        expr_value_type(elem, local_types, local_value_types, non_pointer_locals)
                    else {
                        return Some(Type::Array(Box::new(Type::Any)));
                    };
                    match &elem_ty {
                        None => elem_ty = Some(ty),
                        Some(existing) if existing == &ty => {}
                        Some(_) => return Some(Type::Array(Box::new(Type::Any))),
                    }
                }
                Some(Type::Array(Box::new(elem_ty.unwrap_or(Type::Any))))
            }
            Expr::IndexGet { object, .. } => {
                match expr_value_type(object, local_types, local_value_types, non_pointer_locals)? {
                    Type::Array(elem) => Some(*elem),
                    Type::String => Some(Type::String),
                    _ => None,
                }
            }
            Expr::BufferAlloc { .. }
            | Expr::BufferAllocUnsafe(_)
            | Expr::BufferFrom { .. }
            | Expr::BufferFromArrayBuffer { .. }
            | Expr::BufferConcat(_)
            | Expr::Uint8ArrayNew(_)
            | Expr::Uint8ArrayFrom(_)
            | Expr::TextEncoderEncode(_) => Some(Type::Named("Uint8Array".into())),
            Expr::Void(_) => Some(Type::Void),
            _ => None,
        }
    }

    fn expr_is_known_non_pointer(
        expr: &Expr,
        local_types: &HashMap<u32, Type>,
        local_value_types: &HashMap<u32, Type>,
        non_pointer_locals: &HashSet<u32>,
    ) -> bool {
        expr_value_type(expr, local_types, local_value_types, non_pointer_locals)
            .is_some_and(|ty| is_definitely_non_pointer_type(&ty))
    }

    fn collect_expr_writes(expr: &Expr, writes: &mut HashMap<u32, Vec<LocalWrite>>) {
        match expr {
            Expr::LocalSet(id, rhs) => {
                writes
                    .entry(*id)
                    .or_default()
                    .push(LocalWrite::Expr((**rhs).clone()));
                collect_expr_writes(rhs, writes);
            }
            Expr::Update { id, .. } => {
                writes.entry(*id).or_default().push(LocalWrite::NonPointer);
            }
            _ => {
                perry_hir::walker::walk_expr_children(expr, &mut |child| {
                    collect_expr_writes(child, writes)
                });
            }
        }
    }

    fn collect_facts(
        stmts: &[Stmt],
        local_types: &mut HashMap<u32, Type>,
        writes: &mut HashMap<u32, Vec<LocalWrite>>,
    ) {
        for stmt in stmts {
            match stmt {
                Stmt::Let { id, ty, init, .. } => {
                    local_types.insert(*id, ty.clone());
                    if let Some(init) = init {
                        writes
                            .entry(*id)
                            .or_default()
                            .push(LocalWrite::Expr(init.clone()));
                        collect_expr_writes(init, writes);
                    } else {
                        writes.entry(*id).or_default().push(LocalWrite::NonPointer);
                    }
                }
                Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
                    collect_expr_writes(expr, writes);
                }
                Stmt::Return(None)
                | Stmt::Break
                | Stmt::Continue
                | Stmt::LabeledBreak(_)
                | Stmt::LabeledContinue(_)
                | Stmt::PreallocateBoxes(_) => {}
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    collect_expr_writes(condition, writes);
                    collect_facts(then_branch, local_types, writes);
                    if let Some(else_branch) = else_branch {
                        collect_facts(else_branch, local_types, writes);
                    }
                }
                Stmt::While { condition, body } => {
                    collect_expr_writes(condition, writes);
                    collect_facts(body, local_types, writes);
                }
                Stmt::DoWhile { body, condition } => {
                    collect_facts(body, local_types, writes);
                    collect_expr_writes(condition, writes);
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    if let Some(init) = init {
                        collect_facts(std::slice::from_ref(init.as_ref()), local_types, writes);
                    }
                    if let Some(condition) = condition {
                        collect_expr_writes(condition, writes);
                    }
                    if let Some(update) = update {
                        collect_expr_writes(update, writes);
                    }
                    collect_facts(body, local_types, writes);
                }
                Stmt::Labeled { body, .. } => {
                    collect_facts(std::slice::from_ref(body.as_ref()), local_types, writes);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    collect_facts(body, local_types, writes);
                    if let Some(catch) = catch {
                        if let Some((id, _)) = &catch.param {
                            local_types.insert(*id, Type::Any);
                        }
                        collect_facts(&catch.body, local_types, writes);
                    }
                    if let Some(finally) = finally {
                        collect_facts(finally, local_types, writes);
                    }
                }
                Stmt::Switch {
                    discriminant,
                    cases,
                } => {
                    collect_expr_writes(discriminant, writes);
                    for case in cases {
                        if let Some(test) = &case.test {
                            collect_expr_writes(test, writes);
                        }
                        collect_facts(&case.body, local_types, writes);
                    }
                }
            }
        }
    }

    fn collect_flat_row_aliases(
        stmts: &[Stmt],
        flat_const_ids: &HashSet<u32>,
        out: &mut HashSet<u32>,
    ) {
        for stmt in stmts {
            match stmt {
                Stmt::Let {
                    id,
                    mutable: false,
                    init: Some(Expr::IndexGet { object, .. }),
                    ..
                } => {
                    if let Expr::LocalGet(const_id) = object.as_ref() {
                        if flat_const_ids.contains(const_id) {
                            out.insert(*id);
                        }
                    }
                }
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    collect_flat_row_aliases(then_branch, flat_const_ids, out);
                    if let Some(else_branch) = else_branch {
                        collect_flat_row_aliases(else_branch, flat_const_ids, out);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                    collect_flat_row_aliases(body, flat_const_ids, out);
                }
                Stmt::For { init, body, .. } => {
                    if let Some(init) = init {
                        collect_flat_row_aliases(
                            std::slice::from_ref(init.as_ref()),
                            flat_const_ids,
                            out,
                        );
                    }
                    collect_flat_row_aliases(body, flat_const_ids, out);
                }
                Stmt::Labeled { body, .. } => {
                    collect_flat_row_aliases(
                        std::slice::from_ref(body.as_ref()),
                        flat_const_ids,
                        out,
                    );
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    collect_flat_row_aliases(body, flat_const_ids, out);
                    if let Some(catch) = catch {
                        collect_flat_row_aliases(&catch.body, flat_const_ids, out);
                    }
                    if let Some(finally) = finally {
                        collect_flat_row_aliases(finally, flat_const_ids, out);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for case in cases {
                        collect_flat_row_aliases(&case.body, flat_const_ids, out);
                    }
                }
                _ => {}
            }
        }
    }

    let mut local_types: HashMap<u32, Type> = HashMap::new();
    let mut writes: HashMap<u32, Vec<LocalWrite>> = HashMap::new();
    let mut flat_row_alias_ids: HashSet<u32> = HashSet::new();
    for p in params {
        local_types.insert(p.id, p.ty.clone());
    }
    collect_facts(stmts, &mut local_types, &mut writes);
    collect_flat_row_aliases(stmts, flat_const_ids, &mut flat_row_alias_ids);

    let mut local_value_types: HashMap<u32, Type> = local_types
        .iter()
        .filter_map(|(id, ty)| {
            if !matches!(ty, Type::Any | Type::Unknown) {
                Some((*id, ty.clone()))
            } else {
                None
            }
        })
        .collect();
    let mut non_pointer_locals: HashSet<u32> = local_types
        .iter()
        .filter_map(|(id, ty)| {
            if is_definitely_non_pointer_type(ty) {
                Some(*id)
            } else {
                None
            }
        })
        .collect();

    let mut changed = true;
    while changed {
        changed = false;
        for (id, local_writes) in &writes {
            let mut inferred_ty: Option<Type> = None;
            let mut all_non_pointer = !local_writes.is_empty();
            for write in local_writes {
                let write_ty = match write {
                    LocalWrite::NonPointer => Some(Type::Number),
                    LocalWrite::Expr(expr) => {
                        expr_value_type(expr, &local_types, &local_value_types, &non_pointer_locals)
                    }
                };
                match write_ty {
                    Some(ty) => {
                        all_non_pointer &=
                            is_definitely_non_pointer_type(&ty) || non_pointer_locals.contains(id);
                        match &inferred_ty {
                            None => inferred_ty = Some(ty),
                            Some(existing) if existing == &ty => {}
                            Some(_) => inferred_ty = None,
                        }
                    }
                    None => {
                        all_non_pointer = false;
                        inferred_ty = None;
                    }
                }
            }
            if let Some(ty) = inferred_ty {
                if matches!(local_types.get(id), Some(Type::Any | Type::Unknown))
                    && local_value_types.get(id) != Some(&ty)
                {
                    local_value_types.insert(*id, ty);
                    changed = true;
                }
            }
            if all_non_pointer && non_pointer_locals.insert(*id) {
                changed = true;
            }
        }
    }

    let mut out = std::collections::HashMap::new();
    let mut next_slot: u32 = 0;
    for p in params {
        if is_ptr_typed(&p.ty) && !non_pointer_locals.contains(&p.id) {
            out.insert(p.id, next_slot);
            next_slot += 1;
        }
    }
    fn walk(
        stmts: &[Stmt],
        out: &mut std::collections::HashMap<u32, u32>,
        next_slot: &mut u32,
        non_pointer_locals: &HashSet<u32>,
        flat_row_alias_ids: &HashSet<u32>,
    ) {
        for s in stmts {
            match s {
                Stmt::Let { id, ty, .. }
                    if is_ptr_typed(ty) && !non_pointer_locals.contains(id) =>
                {
                    if !flat_row_alias_ids.contains(id) {
                        out.insert(*id, *next_slot);
                        *next_slot += 1;
                    }
                }
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    walk(
                        then_branch,
                        out,
                        next_slot,
                        non_pointer_locals,
                        flat_row_alias_ids,
                    );
                    if let Some(eb) = else_branch {
                        walk(eb, out, next_slot, non_pointer_locals, flat_row_alias_ids);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                    walk(body, out, next_slot, non_pointer_locals, flat_row_alias_ids);
                }
                Stmt::For { init, body, .. } => {
                    if let Some(i) = init {
                        walk(
                            std::slice::from_ref(i.as_ref()),
                            out,
                            next_slot,
                            non_pointer_locals,
                            flat_row_alias_ids,
                        );
                    }
                    walk(body, out, next_slot, non_pointer_locals, flat_row_alias_ids);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    walk(body, out, next_slot, non_pointer_locals, flat_row_alias_ids);
                    if let Some(c) = catch {
                        if let Some((id, _)) = &c.param {
                            // Catch parameter is implicitly bound;
                            // treat as Any (pointer-possible).
                            if !non_pointer_locals.contains(id) {
                                out.insert(*id, *next_slot);
                                *next_slot += 1;
                            }
                        }
                        walk(
                            &c.body,
                            out,
                            next_slot,
                            non_pointer_locals,
                            flat_row_alias_ids,
                        );
                    }
                    if let Some(fb) = finally {
                        walk(fb, out, next_slot, non_pointer_locals, flat_row_alias_ids);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for c in cases {
                        walk(
                            &c.body,
                            out,
                            next_slot,
                            non_pointer_locals,
                            flat_row_alias_ids,
                        );
                    }
                }
                Stmt::Labeled { body, .. } => walk(
                    std::slice::from_ref(body.as_ref()),
                    out,
                    next_slot,
                    non_pointer_locals,
                    flat_row_alias_ids,
                ),
                _ => {}
            }
        }
    }
    walk(
        stmts,
        &mut out,
        &mut next_slot,
        &non_pointer_locals,
        &flat_row_alias_ids,
    );
    out
}
