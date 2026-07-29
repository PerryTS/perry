//! Analysis functions for HIR expressions and statements.
//!
//! Contains functions for collecting local references, tracking assigned locals,
//! checking `this` usage, and identifying builtin functions.

use crate::types::LocalId;

use crate::ir::*;
use crate::walker::{walk_expr_children, walk_expr_children_mut};

mod builtins;
pub(crate) use builtins::{
    builtin_constructor_length, builtin_global_function_length, builtin_static_function_length,
    is_builtin_function, is_builtin_global_value_name, is_builtin_static_function_member,
};

mod uses_this;
pub(crate) use uses_this::{closure_uses_new_target, closure_uses_this, uses_this_stmt};

mod value_types;
pub use value_types::{infer_expr_type, infer_refinable_expr_type, HirTypeEnv, HirTypeFacts};

/// Whether a function BODY reads the dynamic `this` binding — directly via
/// `Expr::This`, or through a nested arrow that captures it. Plain
/// (non-method) functions resolve `this` through the runtime's
/// `IMPLICIT_THIS` slot, so codegen uses this to decide which bare-call
/// sites must reset that slot to `undefined` for the duration of the call
/// (OrdinaryCallBindThis with no receiver — #3576). Function expressions
/// with their own `this` binding (`captures_this == false` closures) don't
/// propagate, matching `uses_this_expr`.
pub fn body_reads_dynamic_this(stmts: &[Stmt]) -> bool {
    stmts.iter().any(uses_this_stmt)
}

/// Collect every `LocalId` referenced by `expr` (and its sub-expressions).
///
/// Per-variant work focuses on the LocalId-bearing variants (LocalGet,
/// LocalSet.id, Update.id, Array*.array_id, SetAdd.set_id, Closure body for
/// transitive captures). Descent into all other sub-expressions is delegated
/// to `walk_expr_children` — see `perry_hir::walker` for why a single source
/// of truth was extracted from the four pre-existing ad-hoc walkers.
pub fn collect_local_refs_expr(
    expr: &Expr,
    refs: &mut Vec<LocalId>,
    visited: &mut std::collections::HashSet<usize>,
) {
    match expr {
        Expr::LocalGet(id) => {
            refs.push(*id);
            return;
        }
        Expr::LocalSet(id, value) => {
            refs.push(*id);
            collect_local_refs_expr(value, refs, visited);
            return;
        }
        Expr::Update { id, .. } => {
            refs.push(*id);
            return;
        }
        Expr::ArrayPush { array_id, .. }
        | Expr::ArrayPushSpread { array_id, .. }
        | Expr::ArrayUnshift { array_id, .. }
        | Expr::ArraySplice { array_id, .. }
        | Expr::ArrayCopyWithin { array_id, .. } => {
            refs.push(*array_id);
            // Children (`value`, `start`, `delete_count`, `items`, `target`,
            // `end`) descended below via the walker.
        }
        Expr::ArrayPop(array_id) | Expr::ArrayShift(array_id) => {
            refs.push(*array_id);
            return;
        }
        Expr::SetAdd { set_id, .. } => {
            refs.push(*set_id);
            // `value` descended via walker.
        }
        Expr::Closure { body, params, .. } => {
            // Descend into nested closures to find transitive captures.
            // Use visited set to prevent infinite loops on recursive closure
            // references. Param defaults are also part of the closure's
            // observable references.
            for p in params {
                if let Some(d) = &p.default {
                    collect_local_refs_expr(d, refs, visited);
                }
            }
            let key = body as *const _ as usize;
            if !visited.insert(key) {
                return;
            }
            for stmt in body {
                collect_local_refs_stmt(stmt, refs, visited);
            }
            return;
        }
        Expr::GlobalGet(_) => {
            // Global variables aren't captures.
            return;
        }
        _ => {}
    }
    // Descend into all immediate sub-expressions for non-special variants.
    // Exhaustive on Expr — adding a new variant to ir.rs without updating
    // walker.rs is a compile error.
    walk_expr_children(expr, &mut |child| {
        collect_local_refs_expr(child, refs, visited)
    });
}

/// Collect all LocalGet references from a statement
pub fn collect_local_refs_stmt(
    stmt: &Stmt,
    refs: &mut Vec<LocalId>,
    visited: &mut std::collections::HashSet<usize>,
) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(init_expr) = init {
                collect_local_refs_expr(init_expr, refs, visited);
            }
        }
        Stmt::Expr(expr) => {
            collect_local_refs_expr(expr, refs, visited);
        }
        Stmt::Return(expr) => {
            if let Some(e) = expr {
                collect_local_refs_expr(e, refs, visited);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_local_refs_expr(condition, refs, visited);
            for s in then_branch {
                collect_local_refs_stmt(s, refs, visited);
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    collect_local_refs_stmt(s, refs, visited);
                }
            }
        }
        Stmt::While { condition, body } => {
            collect_local_refs_expr(condition, refs, visited);
            for s in body {
                collect_local_refs_stmt(s, refs, visited);
            }
        }
        Stmt::DoWhile { body, condition } => {
            for s in body {
                collect_local_refs_stmt(s, refs, visited);
            }
            collect_local_refs_expr(condition, refs, visited);
        }
        Stmt::Labeled { body, .. } => {
            collect_local_refs_stmt(body, refs, visited);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                collect_local_refs_stmt(init_stmt, refs, visited);
            }
            if let Some(cond) = condition {
                collect_local_refs_expr(cond, refs, visited);
            }
            if let Some(upd) = update {
                collect_local_refs_expr(upd, refs, visited);
            }
            for s in body {
                collect_local_refs_stmt(s, refs, visited);
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_local_refs_stmt(s, refs, visited);
            }
            if let Some(catch_clause) = catch {
                for s in &catch_clause.body {
                    collect_local_refs_stmt(s, refs, visited);
                }
            }
            if let Some(finally_stmts) = finally {
                for s in finally_stmts {
                    collect_local_refs_stmt(s, refs, visited);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_local_refs_expr(discriminant, refs, visited);
            for case in cases {
                if let Some(ref test) = case.test {
                    collect_local_refs_expr(test, refs, visited);
                }
                for s in &case.body {
                    collect_local_refs_stmt(s, refs, visited);
                }
            }
        }
        Stmt::Throw(expr) => {
            collect_local_refs_expr(expr, refs, visited);
        }
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {
            // Pre-allocates slot+box; no expression sub-tree to visit.
        }
    }
}

/// Collect all local IDs that are assigned to in a statement
pub(crate) fn collect_assigned_locals_stmt(stmt: &Stmt, assigned: &mut Vec<LocalId>) {
    match stmt {
        Stmt::Let { .. } => {
            // Let declaration doesn't count as assignment to outer variable
        }
        Stmt::Expr(expr) => {
            collect_assigned_locals_expr(expr, assigned);
        }
        Stmt::Return(expr) => {
            if let Some(e) = expr {
                collect_assigned_locals_expr(e, assigned);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_assigned_locals_expr(condition, assigned);
            for s in then_branch {
                collect_assigned_locals_stmt(s, assigned);
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    collect_assigned_locals_stmt(s, assigned);
                }
            }
        }
        Stmt::While { condition, body } => {
            collect_assigned_locals_expr(condition, assigned);
            for s in body {
                collect_assigned_locals_stmt(s, assigned);
            }
        }
        Stmt::DoWhile { body, condition } => {
            for s in body {
                collect_assigned_locals_stmt(s, assigned);
            }
            collect_assigned_locals_expr(condition, assigned);
        }
        Stmt::Labeled { body, .. } => {
            collect_assigned_locals_stmt(body, assigned);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                collect_assigned_locals_stmt(init_stmt, assigned);
            }
            if let Some(cond) = condition {
                collect_assigned_locals_expr(cond, assigned);
            }
            if let Some(upd) = update {
                collect_assigned_locals_expr(upd, assigned);
            }
            for s in body {
                collect_assigned_locals_stmt(s, assigned);
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_assigned_locals_stmt(s, assigned);
            }
            if let Some(catch_clause) = catch {
                for s in &catch_clause.body {
                    collect_assigned_locals_stmt(s, assigned);
                }
            }
            if let Some(finally_stmts) = finally {
                for s in finally_stmts {
                    collect_assigned_locals_stmt(s, assigned);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_assigned_locals_expr(discriminant, assigned);
            for case in cases {
                if let Some(ref test) = case.test {
                    collect_assigned_locals_expr(test, assigned);
                }
                for s in &case.body {
                    collect_assigned_locals_stmt(s, assigned);
                }
            }
        }
        Stmt::Throw(expr) => {
            collect_assigned_locals_expr(expr, assigned);
        }
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {
            // Slot+box allocation; no assignment to an outer variable.
        }
    }
}

/// Collect all local IDs that are assigned to in an expression
pub(crate) fn collect_assigned_locals_expr(expr: &Expr, assigned: &mut Vec<LocalId>) {
    // #5293: only these arms RECORD an assignment (or intentionally stop
    // descent). Every other variant delegates child descent to the exhaustive
    // `walk_expr_children` below. The previous hand-rolled version enumerated
    // ~100 variants and ended in a `_ => {}` catch-all, so a newly-added Expr
    // variant nesting a `LocalSet`/`Update` silently escaped assignment
    // tracking (the same bug-class as #5143). The walker is exhaustively
    // matched on Expr, so a forgotten variant is a compile error there.
    match expr {
        Expr::LocalSet(id, value) => {
            // This is an assignment to a local variable
            assigned.push(*id);
            collect_assigned_locals_expr(value, assigned);
            return;
        }
        Expr::Update { id, .. } => {
            // Update is an assignment
            assigned.push(*id);
            return;
        }
        // Array methods - push/unshift may reassign the array pointer
        Expr::ArrayPush { array_id, value }
        | Expr::ArrayUnshift { array_id, value }
        | Expr::ArrayPushSpread {
            array_id,
            source: value,
        } => {
            assigned.push(*array_id); // These may reallocate the array
            collect_assigned_locals_expr(value, assigned);
            return;
        }
        Expr::ArraySplice {
            array_id,
            start,
            delete_count,
            items,
        } => {
            assigned.push(*array_id); // Splice may reallocate the array
            collect_assigned_locals_expr(start, assigned);
            if let Some(dc) = delete_count {
                collect_assigned_locals_expr(dc, assigned);
            }
            for item in items {
                collect_assigned_locals_expr(item, assigned);
            }
            return;
        }
        Expr::ArrayReverseValue { receiver } => {
            if let Expr::LocalGet(id) = receiver.as_ref() {
                assigned.push(*id);
            }
            collect_assigned_locals_expr(receiver, assigned);
            return;
        }
        Expr::ArrayCopyWithin {
            array_id,
            target,
            start,
            end,
        } => {
            assigned.push(*array_id); // copyWithin modifies array in-place
            collect_assigned_locals_expr(target, assigned);
            collect_assigned_locals_expr(start, assigned);
            if let Some(e) = end {
                collect_assigned_locals_expr(e, assigned);
            }
            return;
        }
        Expr::SetAdd { set_id, value } => {
            assigned.push(*set_id); // Set is modified by add
            collect_assigned_locals_expr(value, assigned);
            return;
        }
        Expr::Closure { .. } => {
            // Don't recurse into nested closures - assignments there are local
            // to that closure.
            return;
        }
        _ => {}
    }
    walk_expr_children(expr, &mut |child| {
        collect_assigned_locals_expr(child, assigned)
    });
}

/// Rewrite all `Expr::This` references inside a block of statements to
/// `Expr::LocalGet(this_id)`. Used to lift class generator methods
/// (`*[Symbol.iterator]()`) to a top-level function with `this` as an
/// explicit parameter.
///
/// Does NOT recurse into nested closures — those have their own `this`
/// binding and should keep referencing the outer class context.
/// Substitute every LEXICAL `this` in `expr` with `replacement` — including
/// inside arrow / `this`-capturing closure bodies (whose `this` is lexical),
/// but NOT inside ordinary function-expression closures (own dynamic `this`).
///
/// Used by the class-decl static-field-init inline emission: per
/// ClassDefinitionEvaluation a static initializer runs with `this` bound to
/// the class constructor, but the inline `StaticFieldSet` stmts evaluate in
/// module-init context where `this_stack` is empty and `Expr::This` would
/// read the module's implicit `this` (test262 class/elements
/// static-field-init-this-inside-arrow-function, class-name-static-initializer).
pub fn substitute_lexical_this_in_expr(expr: &mut Expr, replacement: &Expr) {
    match expr {
        Expr::This => *expr = replacement.clone(),
        Expr::Closure {
            body,
            captures_this,
            params,
            ..
        } => {
            if *captures_this {
                for p in params.iter_mut() {
                    if let Some(d) = &mut p.default {
                        substitute_lexical_this_in_expr(d, replacement);
                    }
                }
                substitute_lexical_this_in_stmts(body, replacement);
                // The body no longer reads `this`; drop the reserved capture
                // slot so the closure-cache key doesn't include a stale
                // implicit-this snapshot.
                *captures_this = false;
            }
        }
        _ => crate::walker::walk_expr_children_mut(expr, &mut |child| {
            substitute_lexical_this_in_expr(child, replacement)
        }),
    }
}

pub fn substitute_lexical_this_in_stmts(stmts: &mut [Stmt], replacement: &Expr) {
    for s in stmts {
        substitute_lexical_this_in_stmt(s, replacement);
    }
}

fn substitute_lexical_this_in_stmt(stmt: &mut Stmt, replacement: &Expr) {
    let on_expr = |e: &mut Expr| substitute_lexical_this_in_expr(e, replacement);
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                on_expr(e);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => on_expr(e),
        Stmt::Return(e) => {
            if let Some(e) = e {
                on_expr(e);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            on_expr(condition);
            substitute_lexical_this_in_stmts(then_branch, replacement);
            if let Some(eb) = else_branch {
                substitute_lexical_this_in_stmts(eb, replacement);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            on_expr(condition);
            substitute_lexical_this_in_stmts(body, replacement);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                substitute_lexical_this_in_stmt(i, replacement);
            }
            if let Some(c) = condition {
                on_expr(c);
            }
            if let Some(u) = update {
                on_expr(u);
            }
            substitute_lexical_this_in_stmts(body, replacement);
        }
        Stmt::Labeled { body, .. } => substitute_lexical_this_in_stmt(body, replacement),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            substitute_lexical_this_in_stmts(body, replacement);
            if let Some(c) = catch {
                substitute_lexical_this_in_stmts(&mut c.body, replacement);
            }
            if let Some(f) = finally {
                substitute_lexical_this_in_stmts(f, replacement);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            on_expr(discriminant);
            for case in cases {
                if let Some(t) = &mut case.test {
                    substitute_lexical_this_in_expr(t, replacement);
                }
                substitute_lexical_this_in_stmts(&mut case.body, replacement);
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

pub fn replace_this_in_stmts(stmts: &mut Vec<Stmt>, this_id: LocalId) {
    for s in stmts {
        replace_this_in_stmt(s, this_id);
    }
}

/// Issue #212: rewrite every `LocalGet(old_id)` / `LocalSet(old_id, _)` /
/// `Update { id: old_id, .. }` reference (plus the LocalId fields baked
/// into specialized HIR variants like `Expr::ArrayPush { array_id }` and
/// the `captures` / `mutable_captures` lists on `Expr::Closure`) where
/// `old_id` appears as a key in `map`, replacing it with the corresponding
/// `new_id`. Used by `lower_class_decl` to remap captured outer-fn
/// LocalIds onto fresh per-method LocalIds, so the boxed-vars analysis at
/// codegen time scopes each method's box decision to that method (and not
/// to the outer fn's non-boxed slot for the same id).
///
/// The variant coverage mirrors `perry_transform::inline::substitute_locals`
/// (which handles the inliner's full Expr-substitution shape). HIR has
/// hundreds of specialized variants (ArrayJoin, ArrayMap, MathPow, etc.),
/// most of which carry one or more `Box<Expr>` sub-trees that must be
/// recursively rewritten — variants we miss here would silently skip the
/// rewrite and the codegen would fall back to `double_literal(0.0)` (the
/// soft fallback for unrecognized LocalIds), producing an array handle of
/// 0 at runtime. Keep the variant list in sync with `substitute_locals`
/// when adding new HIR shapes.
pub fn remap_local_ids_in_stmts(
    stmts: &mut Vec<Stmt>,
    map: &std::collections::HashMap<LocalId, LocalId>,
) {
    if map.is_empty() {
        return;
    }
    for s in stmts {
        remap_local_ids_in_stmt(s, map);
    }
}

/// Issue #212: like `remap_local_ids_in_stmts` but additionally wraps every
/// `Expr::LocalSet(id, v)` and `Expr::Update { id, .. }` (where `id` is a key
/// in `field_propagation`, BEFORE remapping) in a `Sequence` that also writes
/// the new value back to the corresponding `this.<field_name>`. Used by
/// `lower_class_decl` to make method-body mutations of a captured outer
/// local visible across method calls — without this, a setter writing to a
/// captured primitive would only update the method-local rebind slot, and
/// the next getter call would re-read the field's stale snapshot.
///
/// `field_propagation` keys are OUTER LocalIds (pre-remap); values are the
/// `__perry_cap_<id>` field names. The wrapper detects the captured write
/// by inspecting the original id, then runs the standard remap on the
/// LocalSet/Update inside the wrap so the resulting Sequence references the
/// fresh per-method id everywhere consistently.
pub fn remap_local_ids_in_stmts_with_field_propagation(
    stmts: &mut Vec<Stmt>,
    map: &std::collections::HashMap<LocalId, LocalId>,
    field_propagation: &std::collections::HashMap<LocalId, String>,
) {
    if map.is_empty() && field_propagation.is_empty() {
        return;
    }
    for s in stmts {
        remap_local_ids_in_stmt_propagating(s, map, field_propagation);
    }
}

fn remap_local_ids_in_stmt_propagating(
    stmt: &mut Stmt,
    map: &std::collections::HashMap<LocalId, LocalId>,
    fp: &std::collections::HashMap<LocalId, String>,
) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                remap_with_propagation(e, map, fp);
            }
        }
        Stmt::Expr(e) => remap_with_propagation(e, map, fp),
        Stmt::Return(Some(e)) => remap_with_propagation(e, map, fp),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            remap_with_propagation(condition, map, fp);
            remap_local_ids_in_stmts_with_field_propagation(then_branch, map, fp);
            if let Some(eb) = else_branch {
                remap_local_ids_in_stmts_with_field_propagation(eb, map, fp);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            remap_with_propagation(condition, map, fp);
            remap_local_ids_in_stmts_with_field_propagation(body, map, fp);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                remap_local_ids_in_stmt_propagating(i, map, fp);
            }
            if let Some(c) = condition {
                remap_with_propagation(c, map, fp);
            }
            if let Some(u) = update {
                remap_with_propagation(u, map, fp);
            }
            remap_local_ids_in_stmts_with_field_propagation(body, map, fp);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            remap_local_ids_in_stmts_with_field_propagation(body, map, fp);
            if let Some(c) = catch {
                remap_local_ids_in_stmts_with_field_propagation(&mut c.body, map, fp);
            }
            if let Some(f) = finally {
                remap_local_ids_in_stmts_with_field_propagation(f, map, fp);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            remap_with_propagation(discriminant, map, fp);
            for c in cases {
                if let Some(t) = &mut c.test {
                    remap_with_propagation(t, map, fp);
                }
                remap_local_ids_in_stmts_with_field_propagation(&mut c.body, map, fp);
            }
        }
        Stmt::Throw(e) => remap_with_propagation(e, map, fp),
        Stmt::Labeled { body, .. } => remap_local_ids_in_stmt_propagating(body, map, fp),
        _ => {}
    }
}

/// Detect captured-LocalSet/Update at this position, replace with a
/// Sequence that also propagates the new value to the field. Then run the
/// standard rename pass on the wrapped expr so all ids inside are fresh.
fn remap_with_propagation(
    expr: &mut Expr,
    map: &std::collections::HashMap<LocalId, LocalId>,
    fp: &std::collections::HashMap<LocalId, String>,
) {
    // Detect captured LocalSet / Update at THIS position. Use the
    // pre-remap (outer) id to look up the field name.
    let captured_field: Option<(LocalId, String)> = match expr {
        Expr::LocalSet(id, _) => fp.get(id).map(|f| (*id, f.clone())),
        Expr::Update { id, .. } => fp.get(id).map(|f| (*id, f.clone())),
        _ => None,
    };
    if let Some((outer_id, field_name)) = captured_field {
        // Pull out the original LocalSet/Update so we can rename its inner
        // ids before rewrapping in a Sequence.
        let mut original = std::mem::replace(expr, Expr::Undefined);
        // Standard remap on the original (without propagation — we're
        // about to manually wrap; recursing back here would loop).
        remap_local_ids_in_expr(&mut original, map);
        // After remap, the LocalSet/Update's id is fresh_id (or unchanged
        // if outer_id wasn't in `map`).
        let fresh_id = *map.get(&outer_id).unwrap_or(&outer_id);
        *expr = Expr::Sequence(vec![
            original,
            Expr::PropertySet {
                object: Box::new(Expr::This),
                property: field_name,
                value: Box::new(Expr::LocalGet(fresh_id)),
            },
        ]);
        return;
    }
    // Not a captured write at this position. Recurse via the standard
    // remap (which handles all sub-Expr positions and inner closure
    // captures lists).
    remap_local_ids_in_expr(expr, map);
}

fn remap_local_ids_in_stmt(stmt: &mut Stmt, map: &std::collections::HashMap<LocalId, LocalId>) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                remap_local_ids_in_expr(e, map);
            }
        }
        Stmt::Expr(e) => remap_local_ids_in_expr(e, map),
        Stmt::Return(Some(e)) => remap_local_ids_in_expr(e, map),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            remap_local_ids_in_expr(condition, map);
            remap_local_ids_in_stmts(then_branch, map);
            if let Some(eb) = else_branch {
                remap_local_ids_in_stmts(eb, map);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            remap_local_ids_in_expr(condition, map);
            remap_local_ids_in_stmts(body, map);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                remap_local_ids_in_stmt(i, map);
            }
            if let Some(c) = condition {
                remap_local_ids_in_expr(c, map);
            }
            if let Some(u) = update {
                remap_local_ids_in_expr(u, map);
            }
            remap_local_ids_in_stmts(body, map);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            remap_local_ids_in_stmts(body, map);
            if let Some(c) = catch {
                remap_local_ids_in_stmts(&mut c.body, map);
            }
            if let Some(f) = finally {
                remap_local_ids_in_stmts(f, map);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            remap_local_ids_in_expr(discriminant, map);
            for c in cases {
                if let Some(t) = &mut c.test {
                    remap_local_ids_in_expr(t, map);
                }
                remap_local_ids_in_stmts(&mut c.body, map);
            }
        }
        Stmt::Throw(e) => remap_local_ids_in_expr(e, map),
        Stmt::Labeled { body, .. } => remap_local_ids_in_stmt(body, map),
        _ => {}
    }
}

/// Apply `map` to every `LocalId` referenced by `expr` (and sub-expressions).
///
/// Per-variant work focuses on the LocalId-bearing variants (LocalGet,
/// LocalSet.id, Update.id, Array*.array_id, SetAdd.set_id, Closure
/// captures lists). Descent into all other sub-expressions is delegated to
/// `walk_expr_children_mut` — the central exhaustive walker in
/// `perry_hir::walker`. Pre-refactor this fn carried its own ad-hoc walker
/// with a `_ => {}` catch-all that silently skipped any new variant added to
/// `Expr` (issue #212 partial-fix lineage).
pub fn remap_local_ids_in_expr(expr: &mut Expr, map: &std::collections::HashMap<LocalId, LocalId>) {
    match expr {
        Expr::LocalGet(id) => {
            if let Some(&new_id) = map.get(id) {
                *id = new_id;
            }
            return;
        }
        Expr::LocalSet(id, value) => {
            if let Some(&new_id) = map.get(id) {
                *id = new_id;
            }
            remap_local_ids_in_expr(value, map);
            return;
        }
        Expr::Update { id, .. } => {
            if let Some(&new_id) = map.get(id) {
                *id = new_id;
            }
            return;
        }
        Expr::ArrayPush { array_id, .. }
        | Expr::ArrayPushSpread { array_id, .. }
        | Expr::ArrayUnshift { array_id, .. }
        | Expr::ArraySplice { array_id, .. }
        | Expr::ArrayCopyWithin { array_id, .. } => {
            if let Some(&new_id) = map.get(array_id) {
                *array_id = new_id;
            }
            // Children descended below via the walker.
        }
        Expr::ArrayPop(array_id) | Expr::ArrayShift(array_id) => {
            if let Some(&new_id) = map.get(array_id) {
                *array_id = new_id;
            }
            return;
        }
        Expr::SetAdd { set_id, .. } => {
            if let Some(&new_id) = map.get(set_id) {
                *set_id = new_id;
            }
            // `value` descended via walker.
        }
        Expr::Closure {
            body,
            captures,
            mutable_captures,
            params,
            ..
        } => {
            // Remap the closure's captures lists AND descend into its body.
            // The body's `LocalGet(old_id)` matches the captures list, and
            // both must be remapped together so the creation site (which
            // reads the captured value from the enclosing scope's remapped
            // slot) and the closure body (which reads via the capture slot
            // index) stay aligned.
            for id in captures.iter_mut() {
                if let Some(&new_id) = map.get(id) {
                    *id = new_id;
                }
            }
            for id in mutable_captures.iter_mut() {
                if let Some(&new_id) = map.get(id) {
                    *id = new_id;
                }
            }
            for p in params.iter_mut() {
                if let Some(d) = &mut p.default {
                    remap_local_ids_in_expr(d, map);
                }
            }
            remap_local_ids_in_stmts(body, map);
            return;
        }
        _ => {}
    }
    // Descend into all immediate sub-expressions for non-special variants.
    walk_expr_children_mut(expr, &mut |child| remap_local_ids_in_expr(child, map));
}

fn replace_this_in_stmt(stmt: &mut Stmt, this_id: LocalId) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                replace_this_in_expr(e, this_id);
            }
        }
        Stmt::Expr(e) => replace_this_in_expr(e, this_id),
        Stmt::Return(Some(e)) => replace_this_in_expr(e, this_id),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            replace_this_in_expr(condition, this_id);
            replace_this_in_stmts(then_branch, this_id);
            if let Some(eb) = else_branch {
                replace_this_in_stmts(eb, this_id);
            }
        }
        Stmt::While { condition, body } => {
            replace_this_in_expr(condition, this_id);
            replace_this_in_stmts(body, this_id);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                replace_this_in_stmt(i, this_id);
            }
            if let Some(c) = condition {
                replace_this_in_expr(c, this_id);
            }
            if let Some(u) = update {
                replace_this_in_expr(u, this_id);
            }
            replace_this_in_stmts(body, this_id);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            replace_this_in_stmts(body, this_id);
            if let Some(c) = catch {
                replace_this_in_stmts(&mut c.body, this_id);
            }
            if let Some(f) = finally {
                replace_this_in_stmts(f, this_id);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            replace_this_in_expr(discriminant, this_id);
            for c in cases {
                if let Some(t) = &mut c.test {
                    replace_this_in_expr(t, this_id);
                }
                replace_this_in_stmts(&mut c.body, this_id);
            }
        }
        Stmt::Throw(e) => replace_this_in_expr(e, this_id),
        _ => {}
    }
}

fn replace_this_in_expr(expr: &mut Expr, this_id: LocalId) {
    match expr {
        Expr::This => {
            *expr = Expr::LocalGet(this_id);
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            replace_this_in_expr(left, this_id);
            replace_this_in_expr(right, this_id);
        }
        Expr::Unary { operand, .. } => replace_this_in_expr(operand, this_id),
        Expr::Update { .. } => {}
        Expr::Call { callee, args, .. } => {
            replace_this_in_expr(callee, this_id);
            for a in args {
                replace_this_in_expr(a, this_id);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            replace_this_in_expr(callee, this_id);
            for a in args {
                match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => replace_this_in_expr(e, this_id),
                }
            }
        }
        Expr::PropertyGet { object, .. } => replace_this_in_expr(object, this_id),
        Expr::PropertySet { object, value, .. } => {
            replace_this_in_expr(object, this_id);
            replace_this_in_expr(value, this_id);
        }
        Expr::PropertyUpdate { object, .. } => replace_this_in_expr(object, this_id),
        Expr::IndexGet { object, index } => {
            replace_this_in_expr(object, this_id);
            replace_this_in_expr(index, this_id);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            replace_this_in_expr(object, this_id);
            replace_this_in_expr(index, this_id);
            replace_this_in_expr(value, this_id);
        }
        Expr::IndexUpdate { object, index, .. } => {
            replace_this_in_expr(object, this_id);
            replace_this_in_expr(index, this_id);
        }
        Expr::LocalSet(_, value) => replace_this_in_expr(value, this_id),
        Expr::GlobalSet(_, value) => replace_this_in_expr(value, this_id),
        Expr::New { args, .. } => {
            for a in args {
                replace_this_in_expr(a, this_id);
            }
        }
        Expr::NewDynamic { callee, args, .. } => {
            replace_this_in_expr(callee, this_id);
            for a in args {
                replace_this_in_expr(a, this_id);
            }
        }
        Expr::Array(elements) => {
            for e in elements {
                replace_this_in_expr(e, this_id);
            }
        }
        Expr::ArraySpread(elements) => {
            for el in elements {
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => {
                        replace_this_in_expr(e, this_id)
                    }
                    ArrayElement::Hole => {}
                }
            }
        }
        Expr::Object(fields) => {
            for (_, e) in fields {
                replace_this_in_expr(e, this_id);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, e) in parts {
                replace_this_in_expr(e, this_id);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            replace_this_in_expr(condition, this_id);
            replace_this_in_expr(then_expr, this_id);
            replace_this_in_expr(else_expr, this_id);
        }
        Expr::Await(inner) => replace_this_in_expr(inner, this_id),
        Expr::Yield { value, .. } => {
            if let Some(v) = value {
                replace_this_in_expr(v, this_id);
            }
        }
        Expr::TypeOf(o) | Expr::Void(o) => replace_this_in_expr(o, this_id),
        Expr::InstanceOf {
            expr: inner,
            ty_expr,
            ..
        } => {
            replace_this_in_expr(inner, this_id);
            if let Some(t) = ty_expr {
                replace_this_in_expr(t, this_id);
            }
        }
        Expr::In { property, object } => {
            replace_this_in_expr(property, this_id);
            replace_this_in_expr(object, this_id);
        }
        Expr::Sequence(exprs) => {
            for e in exprs {
                replace_this_in_expr(e, this_id);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                replace_this_in_expr(o, this_id);
            }
            for a in args {
                replace_this_in_expr(a, this_id);
            }
        }
        Expr::StaticMethodCall { args, .. } => {
            for a in args {
                replace_this_in_expr(a, this_id);
            }
        }
        Expr::SuperCall(args) => {
            for a in args {
                replace_this_in_expr(a, this_id);
            }
        }
        Expr::SuperMethodCall { args, .. } => {
            for a in args {
                replace_this_in_expr(a, this_id);
            }
        }
        Expr::ObjectSuperPropertyGet {
            home,
            key,
            receiver,
        } => {
            replace_this_in_expr(home, this_id);
            replace_this_in_expr(key, this_id);
            replace_this_in_expr(receiver, this_id);
        }
        Expr::SuperPropertySet { key, value, .. } => {
            replace_this_in_expr(key, this_id);
            replace_this_in_expr(value, this_id);
        }
        Expr::ObjectSuperPropertySet {
            home,
            key,
            value,
            receiver,
        } => {
            replace_this_in_expr(home, this_id);
            replace_this_in_expr(key, this_id);
            replace_this_in_expr(value, this_id);
            replace_this_in_expr(receiver, this_id);
        }
        Expr::ObjectSuperMethodCall {
            home,
            key,
            receiver,
            args,
        } => {
            replace_this_in_expr(home, this_id);
            replace_this_in_expr(key, this_id);
            replace_this_in_expr(receiver, this_id);
            for a in args {
                replace_this_in_expr(a, this_id);
            }
        }
        Expr::StaticFieldSet { value, .. } => replace_this_in_expr(value, this_id),
        // Don't recurse into nested closures — they have their own
        // `this` binding and should keep their references intact.
        Expr::Closure { .. } => {}
        _ => {}
    }
}
