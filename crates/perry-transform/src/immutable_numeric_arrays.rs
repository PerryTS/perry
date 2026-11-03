//! Fold private numeric literal arrays using their construction and complete
//! use census, never an erased element type. Only own, in-bounds elements are
//! replaced; other keys retain ordinary lookup (including inherited getters).
//! The heap array remains the authoritative object. No runtime table or cache
//! is created. Bounded reads become a small balanced tree of literal values.

use std::collections::{HashMap, HashSet};

use perry_hir::types::LocalId;
use perry_hir::walker::{walk_expr_children, walk_expr_children_mut};
use perry_hir::{BinaryOp, CompareOp, Export, Expr, Module, Stmt, UpdateOp};

use crate::closure_local_inline::{for_each_expr_in_stmt_mut, nested_stmt_lists};
use crate::local_occurrences::{count_module, count_module_occurrences};

const MAX_ELEMENTS: usize = 8;
type Arrays = HashMap<LocalId, Vec<Expr>>;

pub(super) fn run(module: &mut Module) {
    if perry_hir::has_deferred_dynamic_code_sites() {
        return;
    }
    let exported: HashSet<&str> = module
        .exported_objects
        .iter()
        .map(String::as_str)
        .chain(module.exports.iter().filter_map(|e| match e {
            Export::Named { local, .. } => Some(local.as_str()),
            _ => None,
        }))
        .collect();
    let mut arrays: Arrays = module
        .init
        .iter()
        .filter_map(|s| match s {
            Stmt::Let {
                id,
                name,
                mutable: false,
                init: Some(Expr::Array(values, _)),
                ..
            } if !exported.contains(name.as_str())
                && !values.is_empty()
                && values.len() <= MAX_ELEMENTS
                && values
                    .iter()
                    .all(|v| matches!(v, Expr::Integer(_) | Expr::Number(_))) =>
            {
                Some((*id, values.clone()))
            }
            _ => None,
        })
        .collect();
    if arrays.is_empty() {
        return;
    }

    // Compare accepted reads against the exhaustive LocalId census. Aliases,
    // writes, captures, exports and uses hidden in fields/defaults/decorators
    // cannot silently pass an incomplete escape walk.
    let counts = count_module_occurrences(module);
    let mut writes = count_module(module, true);
    // A captured counter may be a cell, rather than an ordinary local. Treat
    // capture metadata as unknown writers even if a lowered closure/class
    // environment no longer expresses its writes as LocalSet(counter).
    for f in module
        .functions
        .iter()
        .chain(module.classes.iter().flat_map(|c| {
            c.constructor
                .iter()
                .chain(&c.methods)
                .chain(&c.static_methods)
                .chain(c.getters.iter().map(|(_, f)| f))
                .chain(c.setters.iter().map(|(_, f)| f))
                .chain(c.computed_members.iter().map(|m| &m.function))
        }))
    {
        for id in &f.captures {
            writes.insert(*id, 2);
        }
    }
    perry_hir::tdz_check::for_each_module_expr(module, &mut |e| match e {
        Expr::Closure {
            captures,
            mutable_captures,
            ..
        } => {
            for id in captures.iter().chain(mutable_captures) {
                writes.insert(*id, 2);
            }
        }
        Expr::RegisterClassCaptures { captures, .. }
        | Expr::RefreshClassExprCaptures { captures, .. } => {
            for value in captures {
                perry_hir::tdz_check::for_each_expr_in(value, &mut |e| {
                    if let Expr::LocalGet(id) = e {
                        writes.insert(*id, 2);
                    }
                });
            }
        }
        _ => {}
    });
    let mut allowed = HashMap::<LocalId, usize>::new();
    scan_stmts(
        &module.init,
        &arrays,
        &writes,
        &HashMap::new(),
        &mut allowed,
    );
    for f in &module.functions {
        scan_stmts(&f.body, &arrays, &writes, &HashMap::new(), &mut allowed);
    }
    for c in &module.classes {
        for f in c
            .constructor
            .iter()
            .chain(&c.methods)
            .chain(&c.static_methods)
            .chain(c.getters.iter().map(|(_, f)| f))
            .chain(c.setters.iter().map(|(_, f)| f))
            .chain(c.computed_members.iter().map(|m| &m.function))
        {
            scan_stmts(&f.body, &arrays, &writes, &HashMap::new(), &mut allowed);
        }
    }
    arrays.retain(|id, _| {
        !writes.contains_key(id)
            && counts.get(id).copied() == Some(1 + allowed.get(id).copied().unwrap_or(0))
    });
    if arrays.is_empty() {
        return;
    }

    // Existing TDZ checks remain in the expression, including calls made
    // before a module initializer. A bare read is already proven initialized
    // by HIR lowering. Keep all declarations and capture layouts unchanged.
    rewrite_stmts(&mut module.init, &arrays, &writes, &HashMap::new());
    for f in &mut module.functions {
        rewrite_stmts(&mut f.body, &arrays, &writes, &HashMap::new());
    }
    for c in &mut module.classes {
        for f in c
            .constructor
            .iter_mut()
            .chain(&mut c.methods)
            .chain(&mut c.static_methods)
            .chain(c.getters.iter_mut().map(|(_, f)| f))
            .chain(c.setters.iter_mut().map(|(_, f)| f))
            .chain(c.computed_members.iter_mut().map(|m| &mut m.function))
        {
            rewrite_stmts(&mut f.body, &arrays, &writes, &HashMap::new());
        }
    }
}

fn integer(e: &Expr) -> Option<i64> {
    match e {
        Expr::Integer(n) => Some(*n),
        Expr::Number(n) if n.is_finite() && n.fract() == 0.0 && n.abs() <= i32::MAX as f64 => {
            Some(*n as i64)
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct IndexRange {
    first: i64,
    last: i64,
    // A live bound only proves reads of this SAME array. Otherwise rejecting
    // the bound's array could leave another candidate with a circular proof.
    bound_array: Option<LocalId>,
}
type Ranges = HashMap<LocalId, IndexRange>;

fn loop_range(
    s: &Stmt,
    writes: &HashMap<LocalId, usize>,
    arrays: &Arrays,
) -> Option<(LocalId, IndexRange)> {
    let Stmt::For {
        init: Some(init),
        condition: Some(condition),
        update: Some(update),
        ..
    } = s
    else {
        return None;
    };
    let Stmt::Let {
        id,
        init: Some(start),
        ..
    } = init.as_ref()
    else {
        return None;
    };
    let lo = integer(start)?;
    let Expr::Compare {
        op: CompareOp::Lt,
        left,
        right,
    } = condition
    else {
        return None;
    };
    if !matches!(left.as_ref(), Expr::LocalGet(i) if i == id) {
        return None;
    }
    let bound = perry_hir::tdz_check::without_checks(right);
    let (hi, bound_array) = if let Some(n) = integer(bound) {
        (n, None)
    } else if let Expr::PropertyGet {
        object, property, ..
    } = bound
    {
        let Expr::LocalGet(array) = perry_hir::tdz_check::without_checks(object) else {
            return None;
        };
        if property != "length" {
            return None;
        }
        (arrays.get(array)?.len() as i64, Some(*array))
    } else {
        return None;
    };
    // Small exact integers, with no other writer anywhere (including nested
    // closures), establish an induction proof independent of annotations.
    if lo < 0 || hi > MAX_ELEMENTS as i64 || lo >= hi || writes.get(id) != Some(&1) {
        return None;
    }
    let increments = match update {
        Expr::Update {
            id: i,
            op: UpdateOp::Increment,
            ..
        } => i == id,
        Expr::LocalSet(i, value) if i == id => matches!(value.as_ref(),
            Expr::Binary { op: BinaryOp::Add, left, right }
                if matches!(left.as_ref(), Expr::LocalGet(i) if i == id) && integer(right) == Some(1)),
        _ => false,
    };
    increments.then_some((
        *id,
        IndexRange {
            first: lo,
            last: hi - 1,
            bound_array,
        },
    ))
}

fn own_index(index: &Expr, array: LocalId, len: usize, ranges: &Ranges) -> bool {
    if let Some(n) = integer(index) {
        return n >= 0 && n < len as i64;
    }
    matches!(index, Expr::LocalGet(id) if ranges.get(id).is_some_and(|r|
        r.first >= 0 && r.last < len as i64 && r.bound_array.is_none_or(|bound| bound == array)))
}

fn scan_expr(e: &Expr, arrays: &Arrays, ranges: &Ranges, allowed: &mut HashMap<LocalId, usize>) {
    // An inherited indexed getter receives the array as `this` and can
    // mutate/escape it. Consequently EVERY read must be an own element, not
    // just the reads we intend to replace. Delete is an lvalue, not a read.
    if matches!(e, Expr::Closure { .. } | Expr::Delete(_)) {
        return;
    }
    if let Some(id) = perry_hir::tdz_check::checked_binding(e) {
        *allowed.entry(id).or_default() += 1;
        return;
    }
    let receiver = match e {
        Expr::IndexGet { object, index, .. } => {
            match perry_hir::tdz_check::without_checks(object) {
                Expr::LocalGet(id)
                    if arrays
                        .get(id)
                        .is_some_and(|values| own_index(index, *id, values.len(), ranges)) =>
                {
                    Some(*id)
                }
                _ => None,
            }
        }
        Expr::PropertyGet {
            object, property, ..
        } if property == "length" => match perry_hir::tdz_check::without_checks(object) {
            Expr::LocalGet(id) => Some(*id),
            _ => None,
        },
        _ => None,
    };
    if let Some(id) = receiver {
        *allowed.entry(id).or_default() += 1;
    }
    walk_expr_children(e, &mut |child| scan_expr(child, arrays, ranges, allowed));
}

fn scan_stmts(
    stmts: &[Stmt],
    arrays: &Arrays,
    writes: &HashMap<LocalId, usize>,
    ranges: &Ranges,
    allowed: &mut HashMap<LocalId, usize>,
) {
    for s in stmts {
        let mut expr = |e: &Expr| scan_expr(e, arrays, ranges, allowed);
        match s {
            Stmt::Let { init, .. } | Stmt::Return(init) => {
                if let Some(e) = init {
                    expr(e);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => expr(e),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                expr(condition);
                scan_stmts(then_branch, arrays, writes, ranges, allowed);
                if let Some(body) = else_branch {
                    scan_stmts(body, arrays, writes, ranges, allowed);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                expr(condition);
                scan_stmts(body, arrays, writes, ranges, allowed);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(e) = condition {
                    expr(e);
                }
                if let Some(e) = update {
                    expr(e);
                }
                if let Some(init) = init {
                    scan_stmts(
                        std::slice::from_ref(init.as_ref()),
                        arrays,
                        writes,
                        ranges,
                        allowed,
                    );
                }
                let mut inner = ranges.clone();
                if let Some((id, range)) = loop_range(s, writes, arrays) {
                    inner.insert(id, range);
                }
                scan_stmts(body, arrays, writes, &inner, allowed);
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                expr(discriminant);
                for case in cases {
                    if let Some(e) = &case.test {
                        scan_expr(e, arrays, ranges, allowed);
                    }
                    scan_stmts(&case.body, arrays, writes, ranges, allowed);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                scan_stmts(body, arrays, writes, ranges, allowed);
                if let Some(catch) = catch {
                    scan_stmts(&catch.body, arrays, writes, ranges, allowed);
                }
                if let Some(body) = finally {
                    scan_stmts(body, arrays, writes, ranges, allowed);
                }
            }
            Stmt::Labeled { body, .. } => scan_stmts(
                std::slice::from_ref(body.as_ref()),
                arrays,
                writes,
                ranges,
                allowed,
            ),
            _ => {}
        }
    }
}

fn rewrite_stmts(
    stmts: &mut [Stmt],
    arrays: &Arrays,
    writes: &HashMap<LocalId, usize>,
    ranges: &Ranges,
) {
    for s in stmts {
        if let Stmt::Labeled { body, .. } = s {
            rewrite_stmts(std::slice::from_mut(body.as_mut()), arrays, writes, ranges);
            continue;
        }
        let mut inner_ranges = ranges.clone();
        if let Some((id, range)) = loop_range(s, writes, arrays) {
            inner_ranges.insert(id, range);
        }
        for_each_expr_in_stmt_mut(s, &mut |e| rewrite_expr(e, arrays, ranges));
        for body in nested_stmt_lists(s) {
            rewrite_stmts(body, arrays, writes, &inner_ranges);
        }
    }
}

fn with_checks(receiver: &Expr, value: Expr) -> Expr {
    match receiver {
        Expr::Sequence(parts)
            if parts.len() == 2 && perry_hir::tdz_check::is_read_check(&parts[0]) =>
        {
            Expr::Sequence(vec![parts[0].clone(), with_checks(&parts[1], value)])
        }
        _ => value,
    }
}

fn lookup(values: &[Expr], base: i64, index: LocalId) -> Expr {
    if values.len() == 1 {
        return values[0].clone();
    }
    let middle = values.len() / 2;
    Expr::Conditional {
        condition: Box::new(Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(Expr::LocalGet(index)),
            right: Box::new(Expr::Integer(base + middle as i64)),
        }),
        then_expr: Box::new(lookup(&values[..middle], base, index)),
        else_expr: Box::new(lookup(&values[middle..], base + middle as i64, index)),
    }
}

fn rewrite_expr(e: &mut Expr, arrays: &Arrays, ranges: &Ranges) {
    // A closure executes later; its reads cannot inherit this loop's range.
    if matches!(e, Expr::Closure { .. } | Expr::Delete(_))
        || perry_hir::tdz_check::checked_binding(e).is_some()
    {
        return;
    }
    walk_expr_children_mut(e, &mut |child| rewrite_expr(child, arrays, ranges));
    let (object, index) = match e {
        Expr::PropertyGet {
            object, property, ..
        } if property == "length" => (object, None),
        Expr::IndexGet { object, index, .. } => (object, Some(index)),
        _ => return,
    };
    let Expr::LocalGet(id) = perry_hir::tdz_check::without_checks(object) else {
        return;
    };
    let Some(values) = arrays.get(id) else {
        return;
    };
    let value = match index {
        None => Expr::Integer(values.len() as i64),
        Some(index) => {
            if let Some(n) = integer(index) {
                let Some(value) = usize::try_from(n).ok().and_then(|n| values.get(n)) else {
                    return;
                };
                value.clone()
            } else if let Expr::LocalGet(i) = index.as_ref() {
                let Some(range) = ranges.get(i) else {
                    return;
                };
                if !own_index(index, *id, values.len(), ranges) {
                    return;
                }
                lookup(
                    &values[range.first as usize..=range.last as usize],
                    range.first,
                    *i,
                )
            } else {
                return;
            }
        }
    };
    *e = with_checks(object, value);
}

#[cfg(test)]
#[path = "immutable_numeric_arrays_tests.rs"]
mod tests;
