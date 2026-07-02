//! Escape-check passes for scalar replacement of `new` locals: find
//! candidates and walk every use to mark which ones escape.
//!
//! Split out of `escape_news.rs` in v0.5.1021 to satisfy the file-size CI
//! gate. No behavior change — these functions remain `pub` and are re-
//! exported from `collectors/mod.rs`.

use std::collections::HashSet;

use super::*;

/// Pass 1: walk Stmt tree, find `Let { id, init: New { class_name } }`
/// where id is not boxed/global.
pub fn find_new_candidates(
    stmts: &[perry_hir::Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &std::collections::HashMap<u32, String>,
    candidates: &mut std::collections::HashMap<u32, String>,
) {
    use perry_hir::{Expr, Stmt};
    for s in stmts {
        match s {
            Stmt::Let {
                id,
                init: Some(Expr::New { class_name, .. }),
                ..
            } if !boxed_vars.contains(id) && !module_globals.contains_key(id) => {
                candidates.insert(*id, class_name.clone());
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                find_new_candidates(then_branch, boxed_vars, module_globals, candidates);
                if let Some(eb) = else_branch {
                    find_new_candidates(eb, boxed_vars, module_globals, candidates);
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    find_new_candidates(
                        std::slice::from_ref(init_stmt),
                        boxed_vars,
                        module_globals,
                        candidates,
                    );
                }
                find_new_candidates(body, boxed_vars, module_globals, candidates);
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                find_new_candidates(body, boxed_vars, module_globals, candidates);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                find_new_candidates(body, boxed_vars, module_globals, candidates);
                if let Some(c) = catch {
                    find_new_candidates(&c.body, boxed_vars, module_globals, candidates);
                }
                if let Some(f) = finally {
                    find_new_candidates(f, boxed_vars, module_globals, candidates);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    find_new_candidates(&c.body, boxed_vars, module_globals, candidates);
                }
            }
            Stmt::Labeled { body, .. } => {
                find_new_candidates(
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

/// Pass 2: walk all stmts/exprs checking every use of each candidate.
pub fn check_escapes_in_stmts(
    stmts: &[perry_hir::Stmt],
    candidates: &std::collections::HashMap<u32, String>,
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    escaped: &mut HashSet<u32>,
) {
    use perry_hir::Stmt;
    for s in stmts {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) => {
                check_escapes_in_expr(e, candidates, classes, escaped)
            }
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    check_escapes_in_expr(e, candidates, classes, escaped);
                }
            }
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    check_escapes_in_expr(e, candidates, classes, escaped);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_escapes_in_expr(condition, candidates, classes, escaped);
                check_escapes_in_stmts(then_branch, candidates, classes, escaped);
                if let Some(eb) = else_branch {
                    check_escapes_in_stmts(eb, candidates, classes, escaped);
                }
            }
            Stmt::While { condition, body } => {
                check_escapes_in_expr(condition, candidates, classes, escaped);
                check_escapes_in_stmts(body, candidates, classes, escaped);
            }
            Stmt::DoWhile { body, condition } => {
                check_escapes_in_stmts(body, candidates, classes, escaped);
                check_escapes_in_expr(condition, candidates, classes, escaped);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    check_escapes_in_stmts(
                        std::slice::from_ref(init_stmt),
                        candidates,
                        classes,
                        escaped,
                    );
                }
                if let Some(cond) = condition {
                    check_escapes_in_expr(cond, candidates, classes, escaped);
                }
                if let Some(upd) = update {
                    check_escapes_in_expr(upd, candidates, classes, escaped);
                }
                check_escapes_in_stmts(body, candidates, classes, escaped);
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                check_escapes_in_expr(discriminant, candidates, classes, escaped);
                for case in cases {
                    if let Some(test) = &case.test {
                        check_escapes_in_expr(test, candidates, classes, escaped);
                    }
                    check_escapes_in_stmts(&case.body, candidates, classes, escaped);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                check_escapes_in_stmts(body, candidates, classes, escaped);
                if let Some(c) = catch {
                    check_escapes_in_stmts(&c.body, candidates, classes, escaped);
                }
                if let Some(f) = finally {
                    check_escapes_in_stmts(f, candidates, classes, escaped);
                }
            }
            Stmt::Labeled { body, .. } => {
                check_escapes_in_stmts(
                    std::slice::from_ref(body.as_ref()),
                    candidates,
                    classes,
                    escaped,
                );
            }
            _ => {}
        }
    }
}

/// Check whether a candidate local escapes through the given expression.
///
/// A `LocalGet(id)` is SAFE only if it appears in:
///   - `PropertyGet { object: LocalGet(id), property }` — reading a field
///   - `PropertySet { object: LocalGet(id), property, value }` — writing a
///     field (but value must NOT contain LocalGet(id))
///   - `PropertyUpdate { object: LocalGet(id), .. }` — incrementing a field
///
/// `LocalSet(id, _)` anywhere marks it as escaped (reassignment).
///
/// Any other occurrence of `LocalGet(id)` marks it as escaped.
pub fn check_escapes_in_expr(
    e: &perry_hir::Expr,
    candidates: &std::collections::HashMap<u32, String>,
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    escaped: &mut HashSet<u32>,
) {
    use perry_hir::{ArrayElement, CallArg, Expr};

    match e {
        // Safe uses: PropertyGet on a candidate local — *unless* the
        // property is a getter on the candidate's class. A getter is
        // dispatched as a real method call that takes `this` as a
        // function arg, so the receiver MUST be a real heap pointer,
        // not the scalar-replaced field set. Without this check,
        // `let r = new C(...); r.gettableProp` keeps `r` scalar-
        // replaced, the constructor never runs (its body is folded
        // into per-field stores), and the getter's `this_arg` reads
        // an uninitialized alloca → segfault. (Method calls are
        // already covered: they're wrapped in `Expr::Call` and the
        // Call/CallSpread arms below mark the receiver escaped.)
        Expr::PropertyGet { object, property } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(class_name) = candidates.get(id) {
                    if is_class_getter(classes, class_name, property) {
                        escaped.insert(*id);
                        return;
                    }
                    // Plain field read — safe, don't recurse into object.
                    return;
                }
            }
            // Not a candidate or not a LocalGet — recurse normally
            check_escapes_in_expr(object, candidates, classes, escaped);
        }

        // Safe uses: PropertySet on a candidate local — *unless* the
        // property is a setter (which dispatches as a real method call
        // and needs a heap-resident `this`). Otherwise treat as a plain
        // field write; value must not self-reference the candidate.
        Expr::PropertySet {
            object,
            value,
            property,
        } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(class_name) = candidates.get(id) {
                    if is_class_setter(classes, class_name, property) {
                        escaped.insert(*id);
                        check_escapes_in_expr(value, candidates, classes, escaped);
                        return;
                    }
                    // Object position is safe. But check if value contains
                    // LocalGet(id) — that would be self-referential escape.
                    if expr_contains_local_get(value, *id) {
                        escaped.insert(*id);
                    }
                    // Walk value for OTHER candidate escapes
                    check_escapes_in_expr(value, candidates, classes, escaped);
                    return;
                }
            }
            check_escapes_in_expr(object, candidates, classes, escaped);
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } => {
            if let (Expr::LocalGet(id), Expr::LocalGet(receiver_id), Expr::String(property)) =
                (target.as_ref(), receiver.as_ref(), key.as_ref())
            {
                if id == receiver_id {
                    if let Some(class_name) = candidates.get(id) {
                        if is_class_setter(classes, class_name, property) {
                            escaped.insert(*id);
                            check_escapes_in_expr(value, candidates, classes, escaped);
                            return;
                        }
                        if expr_contains_local_get(value, *id) {
                            escaped.insert(*id);
                        }
                        check_escapes_in_expr(value, candidates, classes, escaped);
                        return;
                    }
                }
            }
            check_escapes_in_expr(target, candidates, classes, escaped);
            check_escapes_in_expr(key, candidates, classes, escaped);
            check_escapes_in_expr(value, candidates, classes, escaped);
            check_escapes_in_expr(receiver, candidates, classes, escaped);
        }

        // Safe uses: PropertyUpdate on a candidate local — *unless* the
        // property is a getter+setter pair (both fire on `obj.x++`).
        Expr::PropertyUpdate {
            object, property, ..
        } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(class_name) = candidates.get(id) {
                    if is_class_getter(classes, class_name, property)
                        || is_class_setter(classes, class_name, property)
                    {
                        escaped.insert(*id);
                        return;
                    }
                    // Safe — field increment on a non-escaping local
                    return;
                }
            }
            check_escapes_in_expr(object, candidates, classes, escaped);
        }

        // LocalSet: reassignment — always an escape
        Expr::LocalSet(id, value) => {
            if candidates.contains_key(id) {
                escaped.insert(*id);
            }
            check_escapes_in_expr(value, candidates, classes, escaped);
        }

        // LocalGet in any OTHER position (not already handled above) = escape
        Expr::LocalGet(id) => {
            if candidates.contains_key(id) {
                escaped.insert(*id);
            }
        }

        // New { args } — the New is the definition site for the candidate,
        // but args can escape OTHER candidates
        Expr::New { args, .. } => {
            for a in args {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
        }

        // Closure bodies: LocalGet(id) inside a closure is always an escape
        // because the closure can outlive the stack frame
        Expr::Closure { body, captures, .. } => {
            // Any captured candidate is an escape
            for c in captures {
                if candidates.contains_key(c) {
                    escaped.insert(*c);
                }
            }
            // Walk body too — closures can reference locals without explicitly
            // listing them in captures (the capture list may be incomplete at
            // this stage)
            check_escapes_in_stmts(body, candidates, classes, escaped);
        }

        // ── Recurse into all sub-expressions ──
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            check_escapes_in_expr(left, candidates, classes, escaped);
            check_escapes_in_expr(right, candidates, classes, escaped);
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
        | Expr::ParseFloat(operand)
        | Expr::ObjectKeys(operand)
        | Expr::ForInKeys(operand)
        | Expr::ObjectValues(operand)
        | Expr::ObjectEntries(operand)
        | Expr::SetSize(operand)
        | Expr::MathSqrt(operand)
        | Expr::MathFloor(operand)
        | Expr::MathCeil(operand)
        | Expr::MathRound(operand)
        | Expr::MathTrunc(operand)
        | Expr::MathSign(operand)
        | Expr::MathAbs(operand)
        | Expr::MathF16round(operand)
        | Expr::MathMinSpread(operand)
        | Expr::MathMaxSpread(operand)
        | Expr::ArrayFrom(operand)
        | Expr::ArrayFromArrayLikeHoley(operand)
        | Expr::IteratorFrom(operand)
        | Expr::Uint8ArrayFrom(operand)
        | Expr::JsonParse(operand)
        | Expr::JsonStringify(operand)
        | Expr::JsonRawJson(operand)
        | Expr::JsonIsRawJson(operand)
        | Expr::IteratorToArray(operand)
        | Expr::GetIterator(operand)
        | Expr::GetAsyncIterator(operand)
        | Expr::ForOfToArray(operand)
        | Expr::ForAwaitToArray(operand)
        | Expr::WeakRefNew(operand)
        | Expr::WeakRefDeref(operand)
        | Expr::FinalizationRegistryNew(operand)
        | Expr::QueueMicrotask(operand)
        | Expr::ArrayIsArray(operand) => {
            check_escapes_in_expr(operand, candidates, classes, escaped);
        }
        Expr::JsonParseTyped { text, .. } => {
            check_escapes_in_expr(text, candidates, classes, escaped);
        }
        Expr::StructuredClone { value, options } => {
            check_escapes_in_expr(value, candidates, classes, escaped);
            check_escapes_in_expr(options, candidates, classes, escaped);
        }
        Expr::ProcessNextTick { callback, args } => {
            check_escapes_in_expr(callback, candidates, classes, escaped);
            for a in args {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            check_escapes_in_expr(condition, candidates, classes, escaped);
            check_escapes_in_expr(then_expr, candidates, classes, escaped);
            check_escapes_in_expr(else_expr, candidates, classes, escaped);
        }
        Expr::Call { callee, args, .. } => {
            // Method-call form: `local.method(...)` needs a real heap `this`
            // pointer unless a conservative method summary proves that the
            // body is just a numeric expression over `this.field`, literals,
            // and fixed numeric params. That summary lets codegen inline the
            // body against scalar field slots instead of dispatching with a
            // heap receiver.
            if let Expr::PropertyGet { object, .. } = callee.as_ref() {
                if let Expr::LocalGet(id) = object.as_ref() {
                    if candidates.contains_key(id) {
                        let is_summarized = if let Expr::PropertyGet { property, .. } =
                            callee.as_ref()
                        {
                            candidates.get(id).is_some_and(|class_name| {
                                crate::collectors::simple_scalar_method_summary(
                                    classes,
                                    class_name,
                                    property,
                                    args.len(),
                                )
                                .is_some()
                            })
                        } else {
                            false
                        };
                        if !is_summarized {
                            escaped.insert(*id);
                        }
                    }
                }
            }
            check_escapes_in_expr(callee, candidates, classes, escaped);
            for a in args {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            if let Expr::PropertyGet { object, .. } = callee.as_ref() {
                if let Expr::LocalGet(id) = object.as_ref() {
                    if candidates.contains_key(id) {
                        escaped.insert(*id);
                    }
                }
            }
            check_escapes_in_expr(callee, candidates, classes, escaped);
            for a in args {
                match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => {
                        check_escapes_in_expr(e, candidates, classes, escaped);
                    }
                }
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                check_escapes_in_expr(o, candidates, classes, escaped);
            }
            for a in args {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
        }
        Expr::IndexGet { object, index } => {
            check_escapes_in_expr(object, candidates, classes, escaped);
            check_escapes_in_expr(index, candidates, classes, escaped);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            check_escapes_in_expr(object, candidates, classes, escaped);
            check_escapes_in_expr(index, candidates, classes, escaped);
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::Array(elements) => {
            for el in elements {
                check_escapes_in_expr(el, candidates, classes, escaped);
            }
        }
        Expr::ArraySpread(elements) => {
            for el in elements {
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => {
                        check_escapes_in_expr(e, candidates, classes, escaped);
                    }
                    ArrayElement::Hole => {}
                }
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                check_escapes_in_expr(v, candidates, classes, escaped);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, e) in parts {
                check_escapes_in_expr(e, candidates, classes, escaped);
            }
        }
        Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArraySome { array, callback }
        | Expr::ArrayEvery { array, callback }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArrayFindLast { array, callback }
        | Expr::ArrayFindLastIndex { array, callback }
        | Expr::ArrayForEach { array, callback }
        | Expr::ArrayFlatMap { array, callback }
        | Expr::ArraySort {
            array,
            comparator: callback,
        } => {
            check_escapes_in_expr(array, candidates, classes, escaped);
            check_escapes_in_expr(callback, candidates, classes, escaped);
        }
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        }
        | Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => {
            check_escapes_in_expr(array, candidates, classes, escaped);
            check_escapes_in_expr(callback, candidates, classes, escaped);
            if let Some(init) = initial {
                check_escapes_in_expr(init, candidates, classes, escaped);
            }
        }
        Expr::ArrayPush { array_id, value } => {
            if candidates.contains_key(array_id) {
                escaped.insert(*array_id);
            }
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
            if candidates.contains_key(id) {
                escaped.insert(*id);
            }
        }
        Expr::ArraySplice {
            array_id,
            start,
            delete_count,
            items,
        } => {
            if candidates.contains_key(array_id) {
                escaped.insert(*array_id);
            }
            check_escapes_in_expr(start, candidates, classes, escaped);
            if let Some(d) = delete_count {
                check_escapes_in_expr(d, candidates, classes, escaped);
            }
            for it in items {
                check_escapes_in_expr(it, candidates, classes, escaped);
            }
        }
        Expr::Sequence(es) => {
            for e in es {
                check_escapes_in_expr(e, candidates, classes, escaped);
            }
        }
        Expr::Update { id, .. } => {
            // Update on a candidate's id means it's being ++/-- directly
            // which would make no sense for an object — mark as escape
            if candidates.contains_key(id) {
                escaped.insert(*id);
            }
        }
        Expr::MapSet { map, key, value } => {
            check_escapes_in_expr(map, candidates, classes, escaped);
            check_escapes_in_expr(key, candidates, classes, escaped);
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::MapGet { map, key } | Expr::MapHas { map, key } | Expr::MapDelete { map, key } => {
            check_escapes_in_expr(map, candidates, classes, escaped);
            check_escapes_in_expr(key, candidates, classes, escaped);
        }
        Expr::SetAdd { set_id, value } => {
            if candidates.contains_key(set_id) {
                escaped.insert(*set_id);
            }
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
            check_escapes_in_expr(set, candidates, classes, escaped);
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::MathPow(a, b)
        | Expr::PathJoin(a, b)
        | Expr::PathWin32Join(a, b)
        | Expr::ObjectIs(a, b)
        | Expr::ObjectHasOwn(a, b) => {
            check_escapes_in_expr(a, candidates, classes, escaped);
            check_escapes_in_expr(b, candidates, classes, escaped);
        }
        Expr::MathMin(values) | Expr::MathMax(values) => {
            for v in values {
                check_escapes_in_expr(v, candidates, classes, escaped);
            }
        }
        Expr::PathWin32 { args, .. } => {
            for v in args {
                check_escapes_in_expr(v, candidates, classes, escaped);
            }
        }
        Expr::ErrorNew(opt) => {
            if let Some(o) = opt {
                check_escapes_in_expr(o, candidates, classes, escaped);
            }
        }
        Expr::ArrayJoin { array, separator } => {
            check_escapes_in_expr(array, candidates, classes, escaped);
            if let Some(sep) = separator {
                check_escapes_in_expr(sep, candidates, classes, escaped);
            }
        }
        Expr::ArraySlice { array, start, end } => {
            check_escapes_in_expr(array, candidates, classes, escaped);
            check_escapes_in_expr(start, candidates, classes, escaped);
            if let Some(e) = end {
                check_escapes_in_expr(e, candidates, classes, escaped);
            }
        }
        Expr::ArrayIncludes {
            array,
            value,
            from_index,
        }
        | Expr::ArrayIndexOf {
            array,
            value,
            from_index,
        }
        | Expr::ArrayLastIndexOf {
            array,
            value,
            from_index,
        } => {
            check_escapes_in_expr(array, candidates, classes, escaped);
            check_escapes_in_expr(value, candidates, classes, escaped);
            if let Some(fi) = from_index {
                check_escapes_in_expr(fi, candidates, classes, escaped);
            }
        }
        Expr::NewDynamic { callee, args, .. } => {
            check_escapes_in_expr(callee, candidates, classes, escaped);
            for a in args {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
        }
        Expr::FetchWithOptions {
            url,
            method,
            body,
            headers,
            headers_dynamic,
            signal,
        } => {
            check_escapes_in_expr(url, candidates, classes, escaped);
            check_escapes_in_expr(method, candidates, classes, escaped);
            check_escapes_in_expr(body, candidates, classes, escaped);
            for (_, v) in headers {
                check_escapes_in_expr(v, candidates, classes, escaped);
            }
            if let Some(hd) = headers_dynamic {
                check_escapes_in_expr(hd, candidates, classes, escaped);
            }
            if let Some(s) = signal {
                check_escapes_in_expr(s, candidates, classes, escaped);
            }
        }
        Expr::SuperCall(args)
        | Expr::StaticMethodCall { args, .. }
        | Expr::SuperMethodCall { args, .. } => {
            for a in args {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
        }
        Expr::ObjectSuperPropertyGet {
            home,
            key,
            receiver,
        } => {
            check_escapes_in_expr(home, candidates, classes, escaped);
            check_escapes_in_expr(key, candidates, classes, escaped);
            check_escapes_in_expr(receiver, candidates, classes, escaped);
        }
        Expr::SuperPropertySet { key, value, .. } => {
            check_escapes_in_expr(key, candidates, classes, escaped);
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::ObjectSuperPropertySet {
            home,
            key,
            value,
            receiver,
        } => {
            check_escapes_in_expr(home, candidates, classes, escaped);
            check_escapes_in_expr(key, candidates, classes, escaped);
            check_escapes_in_expr(value, candidates, classes, escaped);
            check_escapes_in_expr(receiver, candidates, classes, escaped);
        }
        Expr::ObjectSuperMethodCall {
            home,
            key,
            receiver,
            args,
        } => {
            check_escapes_in_expr(home, candidates, classes, escaped);
            check_escapes_in_expr(key, candidates, classes, escaped);
            check_escapes_in_expr(receiver, candidates, classes, escaped);
            for a in args {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
        }
        Expr::I18nString { params, .. } => {
            for (_, v) in params {
                check_escapes_in_expr(v, candidates, classes, escaped);
            }
        }
        Expr::Yield { value, .. } => {
            if let Some(v) = value {
                check_escapes_in_expr(v, candidates, classes, escaped);
            }
        }
        Expr::ParseInt { string, radix } => {
            check_escapes_in_expr(string, candidates, classes, escaped);
            if let Some(r) = radix {
                check_escapes_in_expr(r, candidates, classes, escaped);
            }
        }
        Expr::JsonStringifyFull(value, replacer, indent) => {
            check_escapes_in_expr(value, candidates, classes, escaped);
            check_escapes_in_expr(replacer, candidates, classes, escaped);
            check_escapes_in_expr(indent, candidates, classes, escaped);
        }
        Expr::RegExpTest { regex, string } | Expr::RegExpExec { regex, string } => {
            check_escapes_in_expr(regex, candidates, classes, escaped);
            check_escapes_in_expr(string, candidates, classes, escaped);
        }
        Expr::In { property, object } => {
            check_escapes_in_expr(property, candidates, classes, escaped);
            check_escapes_in_expr(object, candidates, classes, escaped);
        }
        Expr::InstanceOf { expr, .. } => {
            check_escapes_in_expr(expr, candidates, classes, escaped);
        }
        Expr::ObjectRest { object, .. } => {
            check_escapes_in_expr(object, candidates, classes, escaped);
        }
        Expr::StaticFieldSet { value, .. } => {
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::ProcessOn { event, handler } => {
            check_escapes_in_expr(event, candidates, classes, escaped);
            check_escapes_in_expr(handler, candidates, classes, escaped);
        }
        Expr::FsWriteFileSync(a, b)
        | Expr::JsonParseReviver {
            text: a,
            reviver: b,
        }
        | Expr::JsonParseWithReviver(a, b)
        | Expr::PathRelative(a, b)
        | Expr::PathMatchesGlob(a, b)
        | Expr::PathResolveJoin(a, b) => {
            check_escapes_in_expr(a, candidates, classes, escaped);
            check_escapes_in_expr(b, candidates, classes, escaped);
        }
        Expr::FinalizationRegistryRegister {
            registry,
            target,
            held,
            token,
        } => {
            check_escapes_in_expr(registry, candidates, classes, escaped);
            check_escapes_in_expr(target, candidates, classes, escaped);
            check_escapes_in_expr(held, candidates, classes, escaped);
            if let Some(t) = token {
                check_escapes_in_expr(t, candidates, classes, escaped);
            }
        }
        Expr::FinalizationRegistryUnregister { registry, token } => {
            check_escapes_in_expr(registry, candidates, classes, escaped);
            check_escapes_in_expr(token, candidates, classes, escaped);
        }
        Expr::ArrayFromMapped {
            iterable,
            map_fn,
            this_arg,
        } => {
            check_escapes_in_expr(iterable, candidates, classes, escaped);
            check_escapes_in_expr(map_fn, candidates, classes, escaped);
            if let Some(t) = this_arg {
                check_escapes_in_expr(t, candidates, classes, escaped);
            }
        }
        Expr::ObjectGroupBy {
            items: iterable,
            key_fn: map_fn,
        }
        | Expr::MapGroupBy {
            items: iterable,
            key_fn: map_fn,
        } => {
            check_escapes_in_expr(iterable, candidates, classes, escaped);
            check_escapes_in_expr(map_fn, candidates, classes, escaped);
        }
        Expr::ArrayToSorted { array, comparator } => {
            check_escapes_in_expr(array, candidates, classes, escaped);
            if let Some(c) = comparator {
                check_escapes_in_expr(c, candidates, classes, escaped);
            }
        }
        Expr::ArrayToReversed { array }
        | Expr::ArrayReverseValue { receiver: array }
        | Expr::ArrayFlat { array }
        | Expr::ArrayEntries(array)
        | Expr::ArrayKeys(array)
        | Expr::ArrayValues(array) => {
            check_escapes_in_expr(array, candidates, classes, escaped);
        }
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => {
            check_escapes_in_expr(array, candidates, classes, escaped);
            check_escapes_in_expr(start, candidates, classes, escaped);
            check_escapes_in_expr(delete_count, candidates, classes, escaped);
            for it in items {
                check_escapes_in_expr(it, candidates, classes, escaped);
            }
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            check_escapes_in_expr(array, candidates, classes, escaped);
            check_escapes_in_expr(index, candidates, classes, escaped);
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::ArrayCopyWithin {
            target, start, end, ..
        } => {
            check_escapes_in_expr(target, candidates, classes, escaped);
            check_escapes_in_expr(start, candidates, classes, escaped);
            if let Some(e) = end {
                check_escapes_in_expr(e, candidates, classes, escaped);
            }
        }
        Expr::ArrayCopyWithinValue {
            receiver,
            target,
            start,
            end,
        } => {
            check_escapes_in_expr(receiver, candidates, classes, escaped);
            check_escapes_in_expr(target, candidates, classes, escaped);
            check_escapes_in_expr(start, candidates, classes, escaped);
            if let Some(e) = end {
                check_escapes_in_expr(e, candidates, classes, escaped);
            }
        }
        Expr::ArrayAt { array, index } => {
            check_escapes_in_expr(array, candidates, classes, escaped);
            check_escapes_in_expr(index, candidates, classes, escaped);
        }
        Expr::ArrayUnshift { value, .. } => {
            check_escapes_in_expr(value, candidates, classes, escaped);
        }
        Expr::TypedArrayNew { arg, .. } => {
            if let Some(a) = arg {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
        }
        Expr::ChildProcessExecSync { command, options } => {
            check_escapes_in_expr(command, candidates, classes, escaped);
            if let Some(o) = options {
                check_escapes_in_expr(o, candidates, classes, escaped);
            }
        }
        Expr::ChildProcessSpawnSync {
            command,
            args,
            options,
        }
        | Expr::ChildProcessSpawn {
            command,
            args,
            options,
        } => {
            check_escapes_in_expr(command, candidates, classes, escaped);
            if let Some(a) = args {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
            if let Some(o) = options {
                check_escapes_in_expr(o, candidates, classes, escaped);
            }
        }
        Expr::ChildProcessExec {
            command,
            options,
            callback,
        } => {
            check_escapes_in_expr(command, candidates, classes, escaped);
            if let Some(o) = options {
                check_escapes_in_expr(o, candidates, classes, escaped);
            }
            if let Some(c) = callback {
                check_escapes_in_expr(c, candidates, classes, escaped);
            }
        }
        Expr::ChildProcessExecFile {
            file,
            args,
            options,
            callback,
        } => {
            check_escapes_in_expr(file, candidates, classes, escaped);
            if let Some(a) = args {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
            if let Some(o) = options {
                check_escapes_in_expr(o, candidates, classes, escaped);
            }
            if let Some(c) = callback {
                check_escapes_in_expr(c, candidates, classes, escaped);
            }
        }
        Expr::ChildProcessExecFileSync {
            file,
            args,
            options,
        } => {
            check_escapes_in_expr(file, candidates, classes, escaped);
            if let Some(a) = args {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
            if let Some(o) = options {
                check_escapes_in_expr(o, candidates, classes, escaped);
            }
        }
        Expr::ChildProcessSpawnBackground {
            command,
            args,
            log_file,
            env_json,
        } => {
            check_escapes_in_expr(command, candidates, classes, escaped);
            if let Some(a) = args {
                check_escapes_in_expr(a, candidates, classes, escaped);
            }
            check_escapes_in_expr(log_file, candidates, classes, escaped);
            if let Some(e) = env_json {
                check_escapes_in_expr(e, candidates, classes, escaped);
            }
        }
        Expr::ChildProcessGetProcessStatus(h) | Expr::ChildProcessKillProcess(h) => {
            check_escapes_in_expr(h, candidates, classes, escaped);
        }
        Expr::FetchGetWithAuth { url, auth_header } => {
            check_escapes_in_expr(url, candidates, classes, escaped);
            check_escapes_in_expr(auth_header, candidates, classes, escaped);
        }
        Expr::FetchPostWithAuth {
            url,
            auth_header,
            body,
        } => {
            check_escapes_in_expr(url, candidates, classes, escaped);
            check_escapes_in_expr(auth_header, candidates, classes, escaped);
            check_escapes_in_expr(body, candidates, classes, escaped);
        }
        Expr::SetNewFromArray(arr) => check_escapes_in_expr(arr, candidates, classes, escaped),
        Expr::Atob(o) | Expr::Btoa(o) => check_escapes_in_expr(o, candidates, classes, escaped),
        Expr::JsonStringifyPretty {
            value,
            replacer,
            space,
        } => {
            check_escapes_in_expr(value, candidates, classes, escaped);
            if let Some(r) = replacer {
                check_escapes_in_expr(r, candidates, classes, escaped);
            }
            check_escapes_in_expr(space, candidates, classes, escaped);
        }
        Expr::PathBasenameExt(a, b) => {
            check_escapes_in_expr(a, candidates, classes, escaped);
            check_escapes_in_expr(b, candidates, classes, escaped);
        }
        // Leaf expressions that don't contain LocalGet — no escape possible
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
        | Expr::DateNow
        | Expr::PerformanceNow
        | Expr::MapNew
        | Expr::SetNew
        | Expr::EnumMember { .. }
        | Expr::StaticFieldGet { .. }
        | Expr::RegExp { .. }
        | Expr::Uint8ArrayNew(None)
        // #853: `Expr::ErrorNew(opt)` is already matched by the earlier
        // arm (around line 4949). The `ErrorNew(None)` here was dead —
        // removed.
        | Expr::BigInt(_) => {}
        // Catch-all: conservatively mark any candidate referenced in an
        // unrecognized expression as escaped. This is safe — just misses
        // the optimization for patterns we haven't enumerated.
        _ => {
            mark_all_candidate_refs_in_expr(e, candidates, escaped);
        }
    }
}
