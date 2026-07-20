use std::collections::{HashMap, HashSet};

use super::*;

/// Distinguishes a literal-separator string split from an array literal. It
/// must not overlap the valid literal-array length range: a 16-element array
/// is a normal scalar-replacement candidate, not a split sentinel.
const SPLIT_CANDIDATE_MARKER: u32 = u32::MAX;

#[inline]
fn scalar_candidate_allows_index(candidate_len: u32, index: u32) -> bool {
    if candidate_len == SPLIT_CANDIDATE_MARKER {
        index < MAX_SCALAR_ARRAY_LEN as u32
    } else {
        index < candidate_len
    }
}

pub fn collect_non_escaping_arrays(
    stmts: &[perry_hir::Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &std::collections::HashMap<u32, String>,
) -> std::collections::HashMap<u32, u32> {
    let mut candidates: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    find_array_candidates(stmts, boxed_vars, module_globals, &mut candidates);

    if candidates.is_empty() {
        return candidates;
    }

    let mut escaped: HashSet<u32> = HashSet::new();
    check_array_escapes_in_stmts(stmts, &candidates, &mut escaped);

    candidates.retain(|id, _| !escaped.contains(id));
    candidates
}

pub fn collect_non_escaping_array_used_indices(
    stmts: &[perry_hir::Stmt],
    non_escaping_arrays: &HashMap<u32, u32>,
) -> HashMap<u32, HashSet<u32>> {
    let mut used = HashMap::new();
    if non_escaping_arrays.is_empty() {
        return used;
    }
    collect_used_array_indices_in_stmts(stmts, non_escaping_arrays, &mut used);
    used
}

/// Constant array indices whose every use is a direct `.length` read. This
/// lets scalar-replaced string splits compute the part's UTF-16 length without
/// first materializing that part as a temporary JS string.
pub fn collect_non_escaping_array_length_only_indices(
    stmts: &[perry_hir::Stmt],
    non_escaping_arrays: &HashMap<u32, u32>,
) -> HashMap<u32, HashSet<u32>> {
    let mut uses: HashMap<u32, HashMap<u32, (usize, usize)>> = HashMap::new();
    collect_length_only_indices_in_stmts(stmts, non_escaping_arrays, &mut uses);
    uses.into_iter()
        .filter_map(|(id, indices)| {
            let length_only = indices
                .into_iter()
                .filter_map(|(index, (total, length))| (total == length).then_some(index))
                .collect::<HashSet<_>>();
            (!length_only.is_empty()).then_some((id, length_only))
        })
        .collect()
}

fn collect_length_only_indices_in_stmts(
    stmts: &[perry_hir::Stmt],
    non_escaping_arrays: &HashMap<u32, u32>,
    uses: &mut HashMap<u32, HashMap<u32, (usize, usize)>>,
) {
    use perry_hir::Stmt;
    for stmt in stmts {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(expr) = init {
                    collect_length_only_indices_in_expr(expr, non_escaping_arrays, uses);
                }
            }
            Stmt::Expr(expr) | Stmt::Throw(expr) => {
                collect_length_only_indices_in_expr(expr, non_escaping_arrays, uses);
            }
            Stmt::Return(expr) => {
                if let Some(expr) = expr {
                    collect_length_only_indices_in_expr(expr, non_escaping_arrays, uses);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_length_only_indices_in_expr(condition, non_escaping_arrays, uses);
                collect_length_only_indices_in_stmts(then_branch, non_escaping_arrays, uses);
                if let Some(else_branch) = else_branch {
                    collect_length_only_indices_in_stmts(else_branch, non_escaping_arrays, uses);
                }
            }
            Stmt::While { condition, body } => {
                collect_length_only_indices_in_expr(condition, non_escaping_arrays, uses);
                collect_length_only_indices_in_stmts(body, non_escaping_arrays, uses);
            }
            Stmt::DoWhile { body, condition } => {
                collect_length_only_indices_in_stmts(body, non_escaping_arrays, uses);
                collect_length_only_indices_in_expr(condition, non_escaping_arrays, uses);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    collect_length_only_indices_in_stmts(
                        std::slice::from_ref(init.as_ref()),
                        non_escaping_arrays,
                        uses,
                    );
                }
                if let Some(condition) = condition {
                    collect_length_only_indices_in_expr(condition, non_escaping_arrays, uses);
                }
                if let Some(update) = update {
                    collect_length_only_indices_in_expr(update, non_escaping_arrays, uses);
                }
                collect_length_only_indices_in_stmts(body, non_escaping_arrays, uses);
            }
            Stmt::Labeled { body, .. } => collect_length_only_indices_in_stmts(
                std::slice::from_ref(body.as_ref()),
                non_escaping_arrays,
                uses,
            ),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_length_only_indices_in_stmts(body, non_escaping_arrays, uses);
                if let Some(catch) = catch {
                    collect_length_only_indices_in_stmts(&catch.body, non_escaping_arrays, uses);
                }
                if let Some(finally) = finally {
                    collect_length_only_indices_in_stmts(finally, non_escaping_arrays, uses);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                collect_length_only_indices_in_expr(discriminant, non_escaping_arrays, uses);
                for case in cases {
                    if let Some(test) = &case.test {
                        collect_length_only_indices_in_expr(test, non_escaping_arrays, uses);
                    }
                    collect_length_only_indices_in_stmts(&case.body, non_escaping_arrays, uses);
                }
            }
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_) => {}
        }
    }
}

fn collect_length_only_indices_in_expr(
    expr: &perry_hir::Expr,
    non_escaping_arrays: &HashMap<u32, u32>,
    uses: &mut HashMap<u32, HashMap<u32, (usize, usize)>>,
) {
    use perry_hir::Expr;
    if let Expr::IndexGet { object, index } = expr {
        if let (Expr::LocalGet(id), Some(index)) = (object.as_ref(), const_index(index)) {
            if non_escaping_arrays
                .get(id)
                .is_some_and(|&len| scalar_candidate_allows_index(len, index))
            {
                let entry = uses.entry(*id).or_default().entry(index).or_default();
                entry.0 += 1;
            }
        }
    }
    if let Expr::PropertyGet {
        object, property, ..
    } = expr
    {
        if property == "length" {
            if let Expr::IndexGet { object, index } = object.as_ref() {
                if let (Expr::LocalGet(id), Some(index)) = (object.as_ref(), const_index(index)) {
                    if non_escaping_arrays
                        .get(id)
                        .is_some_and(|&len| scalar_candidate_allows_index(len, index))
                    {
                        let entry = uses.entry(*id).or_default().entry(index).or_default();
                        entry.1 += 1;
                    }
                }
            }
        }
    }
    if let Expr::Closure { body, .. } = expr {
        collect_length_only_indices_in_stmts(body, non_escaping_arrays, uses);
    }
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        collect_length_only_indices_in_expr(child, non_escaping_arrays, uses);
    });
}

fn collect_used_array_indices_in_stmts(
    stmts: &[perry_hir::Stmt],
    non_escaping_arrays: &HashMap<u32, u32>,
    used: &mut HashMap<u32, HashSet<u32>>,
) {
    use perry_hir::Stmt;
    for stmt in stmts {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(expr) = init {
                    collect_used_array_indices_in_expr(expr, non_escaping_arrays, used);
                }
            }
            Stmt::Expr(expr) | Stmt::Throw(expr) => {
                collect_used_array_indices_in_expr(expr, non_escaping_arrays, used);
            }
            Stmt::Return(expr) => {
                if let Some(expr) = expr {
                    collect_used_array_indices_in_expr(expr, non_escaping_arrays, used);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_used_array_indices_in_expr(condition, non_escaping_arrays, used);
                collect_used_array_indices_in_stmts(then_branch, non_escaping_arrays, used);
                if let Some(else_branch) = else_branch {
                    collect_used_array_indices_in_stmts(else_branch, non_escaping_arrays, used);
                }
            }
            Stmt::While { condition, body } => {
                collect_used_array_indices_in_expr(condition, non_escaping_arrays, used);
                collect_used_array_indices_in_stmts(body, non_escaping_arrays, used);
            }
            Stmt::DoWhile { body, condition } => {
                collect_used_array_indices_in_stmts(body, non_escaping_arrays, used);
                collect_used_array_indices_in_expr(condition, non_escaping_arrays, used);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    collect_used_array_indices_in_stmts(
                        std::slice::from_ref(init_stmt.as_ref()),
                        non_escaping_arrays,
                        used,
                    );
                }
                if let Some(condition) = condition {
                    collect_used_array_indices_in_expr(condition, non_escaping_arrays, used);
                }
                if let Some(update) = update {
                    collect_used_array_indices_in_expr(update, non_escaping_arrays, used);
                }
                collect_used_array_indices_in_stmts(body, non_escaping_arrays, used);
            }
            Stmt::Labeled { body, .. } => {
                collect_used_array_indices_in_stmts(
                    std::slice::from_ref(body.as_ref()),
                    non_escaping_arrays,
                    used,
                );
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_used_array_indices_in_stmts(body, non_escaping_arrays, used);
                if let Some(catch) = catch {
                    collect_used_array_indices_in_stmts(&catch.body, non_escaping_arrays, used);
                }
                if let Some(finally) = finally {
                    collect_used_array_indices_in_stmts(finally, non_escaping_arrays, used);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                collect_used_array_indices_in_expr(discriminant, non_escaping_arrays, used);
                for case in cases {
                    if let Some(test) = &case.test {
                        collect_used_array_indices_in_expr(test, non_escaping_arrays, used);
                    }
                    collect_used_array_indices_in_stmts(&case.body, non_escaping_arrays, used);
                }
            }
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_) => {}
        }
    }
}

fn collect_used_array_indices_in_expr(
    expr: &perry_hir::Expr,
    non_escaping_arrays: &HashMap<u32, u32>,
    used: &mut HashMap<u32, HashSet<u32>>,
) {
    use perry_hir::Expr;
    if let Expr::IndexGet { object, index } = expr {
        if let Expr::LocalGet(id) = object.as_ref() {
            if let Some(&len) = non_escaping_arrays.get(id) {
                if let Some(k) = const_index(index) {
                    if scalar_candidate_allows_index(len, k) {
                        used.entry(*id).or_default().insert(k);
                    }
                }
            }
        }
    }
    if let Expr::Closure { body, .. } = expr {
        collect_used_array_indices_in_stmts(body, non_escaping_arrays, used);
    }
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        collect_used_array_indices_in_expr(child, non_escaping_arrays, used);
    });
}

pub fn find_array_candidates(
    stmts: &[perry_hir::Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &std::collections::HashMap<u32, String>,
    candidates: &mut std::collections::HashMap<u32, u32>,
) {
    use perry_hir::{Expr, Stmt};
    for s in stmts {
        match s {
            Stmt::Let {
                id,
                init: Some(Expr::Array(elements)),
                ..
            } if !boxed_vars.contains(id) && !module_globals.contains_key(id) => {
                let n = elements.len();
                if (1..=MAX_SCALAR_ARRAY_LEN).contains(&n) {
                    candidates.insert(*id, n as u32);
                }
            }
            // A non-escaping `text.split("literal")` can use the same scalar
            // replacement machinery as an array literal. Its distinct marker
            // still admits only small constant indices; the lowering materializes
            // precisely those slots instead of allocating the result array.
            Stmt::Let {
                id,
                init: Some(Expr::Call { callee, args, .. }),
                ..
            } if !boxed_vars.contains(id)
                && !module_globals.contains_key(id)
                && matches!(
                    callee.as_ref(),
                    Expr::PropertyGet { object, property, .. }
                        if matches!(object.as_ref(), Expr::LocalGet(_))
                            && property == "split"
                )
                && matches!(args.as_slice(), [Expr::String(s)] if !s.is_empty()) =>
            {
                candidates.insert(*id, SPLIT_CANDIDATE_MARKER);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                find_array_candidates(then_branch, boxed_vars, module_globals, candidates);
                if let Some(eb) = else_branch {
                    find_array_candidates(eb, boxed_vars, module_globals, candidates);
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    find_array_candidates(
                        std::slice::from_ref(init_stmt),
                        boxed_vars,
                        module_globals,
                        candidates,
                    );
                }
                find_array_candidates(body, boxed_vars, module_globals, candidates);
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                find_array_candidates(body, boxed_vars, module_globals, candidates);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                find_array_candidates(body, boxed_vars, module_globals, candidates);
                if let Some(c) = catch {
                    find_array_candidates(&c.body, boxed_vars, module_globals, candidates);
                }
                if let Some(f) = finally {
                    find_array_candidates(f, boxed_vars, module_globals, candidates);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    find_array_candidates(&c.body, boxed_vars, module_globals, candidates);
                }
            }
            Stmt::Labeled { body, .. } => {
                find_array_candidates(
                    std::slice::from_ref(body.as_ref()),
                    boxed_vars,
                    module_globals,
                    candidates,
                );
            }
            _ => {}
        }
    }
}

pub fn check_array_escapes_in_stmts(
    stmts: &[perry_hir::Stmt],
    candidates: &std::collections::HashMap<u32, u32>,
    escaped: &mut HashSet<u32>,
) {
    use perry_hir::Stmt;
    for s in stmts {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) => check_array_escapes_in_expr(e, candidates, escaped),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    check_array_escapes_in_expr(e, candidates, escaped);
                }
            }
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    check_array_escapes_in_expr(e, candidates, escaped);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_array_escapes_in_expr(condition, candidates, escaped);
                check_array_escapes_in_stmts(then_branch, candidates, escaped);
                if let Some(eb) = else_branch {
                    check_array_escapes_in_stmts(eb, candidates, escaped);
                }
            }
            Stmt::While { condition, body } => {
                check_array_escapes_in_expr(condition, candidates, escaped);
                check_array_escapes_in_stmts(body, candidates, escaped);
            }
            Stmt::DoWhile { body, condition } => {
                check_array_escapes_in_stmts(body, candidates, escaped);
                check_array_escapes_in_expr(condition, candidates, escaped);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    check_array_escapes_in_stmts(
                        std::slice::from_ref(init_stmt),
                        candidates,
                        escaped,
                    );
                }
                if let Some(cond) = condition {
                    check_array_escapes_in_expr(cond, candidates, escaped);
                }
                if let Some(upd) = update {
                    check_array_escapes_in_expr(upd, candidates, escaped);
                }
                check_array_escapes_in_stmts(body, candidates, escaped);
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                check_array_escapes_in_expr(discriminant, candidates, escaped);
                for case in cases {
                    if let Some(test) = &case.test {
                        check_array_escapes_in_expr(test, candidates, escaped);
                    }
                    check_array_escapes_in_stmts(&case.body, candidates, escaped);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                check_array_escapes_in_stmts(body, candidates, escaped);
                if let Some(c) = catch {
                    check_array_escapes_in_stmts(&c.body, candidates, escaped);
                }
                if let Some(f) = finally {
                    check_array_escapes_in_stmts(f, candidates, escaped);
                }
            }
            Stmt::Labeled { body, .. } => {
                check_array_escapes_in_stmts(
                    std::slice::from_ref(body.as_ref()),
                    candidates,
                    escaped,
                );
            }
            _ => {}
        }
    }
}

/// Extract a non-negative integer from an index expression if and only if it's
/// a compile-time literal that fits in u32. `Integer(k)` and `Number(k)`
/// (when `k` is an exact integer) both count.
pub fn const_index(expr: &perry_hir::Expr) -> Option<u32> {
    use perry_hir::Expr;
    match expr {
        Expr::Integer(k) if *k >= 0 && *k <= u32::MAX as i64 => Some(*k as u32),
        Expr::Number(f)
            if f.is_finite() && *f >= 0.0 && f.fract() == 0.0 && *f <= u32::MAX as f64 =>
        {
            Some(*f as u32)
        }
        _ => None,
    }
}

pub fn check_array_escapes_in_expr(
    e: &perry_hir::Expr,
    candidates: &std::collections::HashMap<u32, u32>,
    escaped: &mut HashSet<u32>,
) {
    use perry_hir::{ArrayElement, CallArg, Expr};

    match e {
        // Safe: constant-index read `arr[k]` where 0 <= k < length.
        Expr::IndexGet { object, index } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(&len) = candidates.get(id) {
                    match const_index(index) {
                        Some(k) if scalar_candidate_allows_index(len, k) => {
                            // Safe use — walk index for other candidates (none
                            // in a literal), skip object walk.
                            check_array_escapes_in_expr(index, candidates, escaped);
                            return;
                        }
                        _ => {
                            // Dynamic or out-of-range index: must keep real array.
                            escaped.insert(*id);
                        }
                    }
                }
            }
            check_array_escapes_in_expr(object, candidates, escaped);
            check_array_escapes_in_expr(index, candidates, escaped);
        }

        // Safe: `arr.length` read folds to the constant N.
        Expr::PropertyGet {
            object, property, ..
        } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(&len) = candidates.get(id) {
                    if property == "length" {
                        // A scalar-replaced literal split has no statically
                        // known runtime length, unlike a literal array.
                        if len == SPLIT_CANDIDATE_MARKER {
                            escaped.insert(*id);
                        } else {
                            return;
                        }
                    }
                }
            }
            check_array_escapes_in_expr(object, candidates, escaped);
        }

        // IndexSet would mutate the array — treat as escape. (Supporting this
        // would require tracking dirty slots and invalidating earlier reads;
        // not worth the complexity for literals that are mostly read-only.)
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if candidates.contains_key(id) {
                    escaped.insert(*id);
                }
            }
            check_array_escapes_in_expr(object, candidates, escaped);
            check_array_escapes_in_expr(index, candidates, escaped);
            check_array_escapes_in_expr(value, candidates, escaped);
        }
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } => {
            if let (Expr::LocalGet(id), Expr::LocalGet(receiver_id)) =
                (target.as_ref(), receiver.as_ref())
            {
                if id == receiver_id && candidates.contains_key(id) {
                    escaped.insert(*id);
                }
            }
            check_array_escapes_in_expr(target, candidates, escaped);
            check_array_escapes_in_expr(key, candidates, escaped);
            check_array_escapes_in_expr(value, candidates, escaped);
            check_array_escapes_in_expr(receiver, candidates, escaped);
        }

        Expr::IndexUpdate { object, index, .. } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if candidates.contains_key(id) {
                    escaped.insert(*id);
                }
            }
            check_array_escapes_in_expr(object, candidates, escaped);
            check_array_escapes_in_expr(index, candidates, escaped);
        }

        // Reassignment is always an escape (and any LocalGet anywhere else).
        Expr::LocalSet(id, value) => {
            if candidates.contains_key(id) {
                escaped.insert(*id);
            }
            check_array_escapes_in_expr(value, candidates, escaped);
        }
        Expr::LocalGet(id) => {
            if candidates.contains_key(id) {
                escaped.insert(*id);
            }
        }

        // Closure captures: if a candidate is captured, it escapes.
        Expr::Closure { body, captures, .. } => {
            for c in captures {
                if candidates.contains_key(c) {
                    escaped.insert(*c);
                }
            }
            check_array_escapes_in_stmts(body, candidates, escaped);
        }

        // ── Recurse into sub-expressions (same structure as object pass). ──
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            check_array_escapes_in_expr(left, candidates, escaped);
            check_array_escapes_in_expr(right, candidates, escaped);
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::Await(operand)
        | Expr::Delete(operand)
        | Expr::StringCoerce(operand)
        | Expr::ObjectCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand)
        | Expr::IsFinite(operand)
        | Expr::IsNaN(operand)
        | Expr::NumberIsNaN(operand)
        | Expr::NumberIsFinite(operand)
        | Expr::NumberIsInteger(operand)
        | Expr::IsUndefinedOrBareNan(operand)
        | Expr::ParseFloat(operand) => {
            check_array_escapes_in_expr(operand, candidates, escaped);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            check_array_escapes_in_expr(condition, candidates, escaped);
            check_array_escapes_in_expr(then_expr, candidates, escaped);
            check_array_escapes_in_expr(else_expr, candidates, escaped);
        }
        Expr::Call { callee, args, .. } => {
            check_array_escapes_in_expr(callee, candidates, escaped);
            for a in args {
                check_array_escapes_in_expr(a, candidates, escaped);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            check_array_escapes_in_expr(callee, candidates, escaped);
            for a in args {
                match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => {
                        check_array_escapes_in_expr(e, candidates, escaped);
                    }
                }
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                check_array_escapes_in_expr(o, candidates, escaped);
            }
            for a in args {
                check_array_escapes_in_expr(a, candidates, escaped);
            }
        }
        Expr::Array(elements) => {
            for el in elements {
                check_array_escapes_in_expr(el, candidates, escaped);
            }
        }
        Expr::ArraySpread(elements) => {
            for el in elements {
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => {
                        check_array_escapes_in_expr(e, candidates, escaped);
                    }
                    ArrayElement::Hole => {}
                }
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                check_array_escapes_in_expr(v, candidates, escaped);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                check_array_escapes_in_expr(a, candidates, escaped);
            }
        }
        Expr::PropertySet { object, value, .. } => {
            check_array_escapes_in_expr(object, candidates, escaped);
            check_array_escapes_in_expr(value, candidates, escaped);
        }
        Expr::PropertyUpdate { object, .. } => {
            check_array_escapes_in_expr(object, candidates, escaped);
        }
        Expr::Sequence(es) => {
            for e in es {
                check_array_escapes_in_expr(e, candidates, escaped);
            }
        }
        Expr::Update { id, .. } => {
            if candidates.contains_key(id) {
                escaped.insert(*id);
            }
        }
        // Leaf expressions: no LocalGet inside.
        Expr::Integer(_)
        | Expr::Number(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Undefined
        | Expr::Null
        | Expr::This
        | Expr::FuncRef(_)
        | Expr::ClassRef(_)
        | Expr::ExternFuncRef { .. }
        | Expr::GlobalGet(_)
        | Expr::BigInt(_) => {}
        // Catch-all: any unrecognized expression conservatively marks every
        // candidate it references as escaped. Safe — we just miss the
        // optimization on patterns we haven't enumerated above.
        _ => {
            let mut refs: HashSet<u32> = HashSet::new();
            collect_ref_ids_in_expr(e, &mut refs);
            for id in refs {
                if candidates.contains_key(&id) {
                    escaped.insert(id);
                }
            }
        }
    }
}

// ── Escape analysis for scalar replacement of non-escaping object literals ──

/// Upper bound on field count — matches `MAX_SCALAR_ARRAY_LEN`. Beyond this the
/// per-field alloca cost overtakes the arena-bump heap path we'd otherwise use.
pub(crate) const MAX_SCALAR_OBJECT_FIELDS: usize = 16;

#[cfg(test)]
mod tests {
    use super::{scalar_candidate_allows_index, SPLIT_CANDIDATE_MARKER};
    use crate::collectors::MAX_SCALAR_ARRAY_LEN;

    #[test]
    fn split_marker_does_not_collide_with_maximum_literal_array_length() {
        let literal_len = MAX_SCALAR_ARRAY_LEN as u32;
        assert!(scalar_candidate_allows_index(literal_len, literal_len - 1));
        assert!(!scalar_candidate_allows_index(literal_len, literal_len));

        assert!(scalar_candidate_allows_index(
            SPLIT_CANDIDATE_MARKER,
            literal_len - 1
        ));
        assert!(!scalar_candidate_allows_index(
            SPLIT_CANDIDATE_MARKER,
            literal_len
        ));
    }
}
