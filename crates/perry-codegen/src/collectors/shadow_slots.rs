pub fn collect_shadow_slot_clear_points(
    stmts: &[perry_hir::Stmt],
    shadow_slot_map: &std::collections::HashMap<u32, u32>,
) -> std::collections::HashMap<usize, Vec<u32>> {
    if shadow_slot_map.is_empty() {
        return std::collections::HashMap::new();
    }

    let mut last_stmt_for_local: std::collections::HashMap<u32, usize> =
        std::collections::HashMap::new();
    for (idx, stmt) in stmts.iter().enumerate() {
        let mut refs = Vec::new();
        let mut visited = std::collections::HashSet::new();
        perry_hir::analysis::collect_local_refs_stmt(stmt, &mut refs, &mut visited);
        collect_declared_shadow_locals_in_stmt(stmt, shadow_slot_map, &mut refs);
        for local_id in refs {
            if shadow_slot_map.contains_key(&local_id) {
                last_stmt_for_local.insert(local_id, idx);
            }
        }
    }

    let mut clear_after: std::collections::HashMap<usize, Vec<u32>> =
        std::collections::HashMap::new();
    for (local_id, stmt_idx) in last_stmt_for_local {
        if let Some(&slot_idx) = shadow_slot_map.get(&local_id) {
            clear_after.entry(stmt_idx).or_default().push(slot_idx);
        }
    }
    for slots in clear_after.values_mut() {
        slots.sort_unstable();
        slots.dedup();
    }
    clear_after
}

pub fn collect_declared_shadow_slots_in_stmts(
    stmts: &[perry_hir::Stmt],
    shadow_slot_map: &std::collections::HashMap<u32, u32>,
) -> Vec<u32> {
    if shadow_slot_map.is_empty() {
        return Vec::new();
    }
    let mut locals = Vec::new();
    for stmt in stmts {
        collect_declared_shadow_locals_in_stmt(stmt, shadow_slot_map, &mut locals);
    }
    let mut slots: Vec<u32> = locals
        .into_iter()
        .filter_map(|local_id| shadow_slot_map.get(&local_id).copied())
        .collect();
    slots.sort_unstable();
    slots.dedup();
    slots
}

pub fn collect_declared_shadow_locals_in_stmt(
    stmt: &perry_hir::Stmt,
    shadow_slot_map: &std::collections::HashMap<u32, u32>,
    out: &mut Vec<u32>,
) {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Let { id, .. } => {
            if shadow_slot_map.contains_key(id) {
                out.push(*id);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            for stmt in then_branch {
                collect_declared_shadow_locals_in_stmt(stmt, shadow_slot_map, out);
            }
            if let Some(else_branch) = else_branch {
                for stmt in else_branch {
                    collect_declared_shadow_locals_in_stmt(stmt, shadow_slot_map, out);
                }
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            for stmt in body {
                collect_declared_shadow_locals_in_stmt(stmt, shadow_slot_map, out);
            }
        }
        Stmt::For { init, body, .. } => {
            if let Some(init) = init {
                collect_declared_shadow_locals_in_stmt(init, shadow_slot_map, out);
            }
            for stmt in body {
                collect_declared_shadow_locals_in_stmt(stmt, shadow_slot_map, out);
            }
        }
        Stmt::Labeled { body, .. } => {
            collect_declared_shadow_locals_in_stmt(body, shadow_slot_map, out);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for stmt in body {
                collect_declared_shadow_locals_in_stmt(stmt, shadow_slot_map, out);
            }
            if let Some(catch) = catch {
                if let Some((id, _)) = &catch.param {
                    if shadow_slot_map.contains_key(id) {
                        out.push(*id);
                    }
                }
                for stmt in &catch.body {
                    collect_declared_shadow_locals_in_stmt(stmt, shadow_slot_map, out);
                }
            }
            if let Some(finally) = finally {
                for stmt in finally {
                    collect_declared_shadow_locals_in_stmt(stmt, shadow_slot_map, out);
                }
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases {
                for stmt in &case.body {
                    collect_declared_shadow_locals_in_stmt(stmt, shadow_slot_map, out);
                }
            }
        }
        Stmt::Expr(_)
        | Stmt::Return(_)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::Throw(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => {}
    }
}
