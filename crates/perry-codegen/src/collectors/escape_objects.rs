use std::collections::{HashMap, HashSet};

use super::*;

pub fn collect_non_escaping_object_literals(
    stmts: &[perry_hir::Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &std::collections::HashMap<u32, String>,
) -> std::collections::HashMap<u32, Vec<String>> {
    let mut candidates: std::collections::HashMap<u32, Vec<String>> =
        std::collections::HashMap::new();
    find_object_literal_candidates(stmts, boxed_vars, module_globals, &mut candidates);

    if candidates.is_empty() {
        return candidates;
    }

    let mut escaped: HashSet<u32> = HashSet::new();
    check_object_literal_escapes_in_stmts(stmts, &candidates, &mut escaped);

    candidates.retain(|id, _| !escaped.contains(id));
    candidates
}

pub fn collect_non_escaping_object_literal_used_fields(
    stmts: &[perry_hir::Stmt],
    non_escaping_object_literals: &HashMap<u32, Vec<String>>,
) -> HashMap<u32, HashSet<String>> {
    let mut used = HashMap::new();
    if non_escaping_object_literals.is_empty() {
        return used;
    }
    collect_used_object_fields_in_stmts(stmts, non_escaping_object_literals, &mut used);
    used
}

fn collect_used_object_fields_in_stmts(
    stmts: &[perry_hir::Stmt],
    non_escaping_object_literals: &HashMap<u32, Vec<String>>,
    used: &mut HashMap<u32, HashSet<String>>,
) {
    use perry_hir::Stmt;
    for stmt in stmts {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(expr) = init {
                    collect_used_object_fields_in_expr(expr, non_escaping_object_literals, used);
                }
            }
            Stmt::Expr(expr) | Stmt::Throw(expr) => {
                collect_used_object_fields_in_expr(expr, non_escaping_object_literals, used);
            }
            Stmt::Return(expr) => {
                if let Some(expr) = expr {
                    collect_used_object_fields_in_expr(expr, non_escaping_object_literals, used);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_used_object_fields_in_expr(condition, non_escaping_object_literals, used);
                collect_used_object_fields_in_stmts(
                    then_branch,
                    non_escaping_object_literals,
                    used,
                );
                if let Some(else_branch) = else_branch {
                    collect_used_object_fields_in_stmts(
                        else_branch,
                        non_escaping_object_literals,
                        used,
                    );
                }
            }
            Stmt::While { condition, body } => {
                collect_used_object_fields_in_expr(condition, non_escaping_object_literals, used);
                collect_used_object_fields_in_stmts(body, non_escaping_object_literals, used);
            }
            Stmt::DoWhile { body, condition } => {
                collect_used_object_fields_in_stmts(body, non_escaping_object_literals, used);
                collect_used_object_fields_in_expr(condition, non_escaping_object_literals, used);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    collect_used_object_fields_in_stmts(
                        std::slice::from_ref(init_stmt.as_ref()),
                        non_escaping_object_literals,
                        used,
                    );
                }
                if let Some(condition) = condition {
                    collect_used_object_fields_in_expr(
                        condition,
                        non_escaping_object_literals,
                        used,
                    );
                }
                if let Some(update) = update {
                    collect_used_object_fields_in_expr(update, non_escaping_object_literals, used);
                }
                collect_used_object_fields_in_stmts(body, non_escaping_object_literals, used);
            }
            Stmt::Labeled { body, .. } => {
                collect_used_object_fields_in_stmts(
                    std::slice::from_ref(body.as_ref()),
                    non_escaping_object_literals,
                    used,
                );
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_used_object_fields_in_stmts(body, non_escaping_object_literals, used);
                if let Some(catch) = catch {
                    collect_used_object_fields_in_stmts(
                        &catch.body,
                        non_escaping_object_literals,
                        used,
                    );
                }
                if let Some(finally) = finally {
                    collect_used_object_fields_in_stmts(
                        finally,
                        non_escaping_object_literals,
                        used,
                    );
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                collect_used_object_fields_in_expr(
                    discriminant,
                    non_escaping_object_literals,
                    used,
                );
                for case in cases {
                    if let Some(test) = &case.test {
                        collect_used_object_fields_in_expr(
                            test,
                            non_escaping_object_literals,
                            used,
                        );
                    }
                    collect_used_object_fields_in_stmts(
                        &case.body,
                        non_escaping_object_literals,
                        used,
                    );
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

fn collect_used_object_fields_in_expr(
    expr: &perry_hir::Expr,
    non_escaping_object_literals: &HashMap<u32, Vec<String>>,
    used: &mut HashMap<u32, HashSet<String>>,
) {
    use perry_hir::Expr;
    if let Expr::PropertyGet { object, property } = expr {
        if let Expr::LocalGet(id) = object.as_ref() {
            if let Some(fields) = non_escaping_object_literals.get(id) {
                if fields.iter().any(|field| field == property) {
                    used.entry(*id).or_default().insert(property.clone());
                }
            }
        }
    }
    if let Expr::PropertyUpdate {
        object, property, ..
    } = expr
    {
        if let Expr::LocalGet(id) = object.as_ref() {
            if let Some(fields) = non_escaping_object_literals.get(id) {
                if fields.iter().any(|field| field == property) {
                    used.entry(*id).or_default().insert(property.clone());
                }
            }
        }
    }
    if let Expr::Closure { body, .. } = expr {
        collect_used_object_fields_in_stmts(body, non_escaping_object_literals, used);
    }
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        collect_used_object_fields_in_expr(child, non_escaping_object_literals, used);
    });
}

pub fn find_object_literal_candidates(
    stmts: &[perry_hir::Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &std::collections::HashMap<u32, String>,
    candidates: &mut std::collections::HashMap<u32, Vec<String>>,
) {
    use perry_hir::{Expr, Stmt};
    for s in stmts {
        match s {
            Stmt::Let {
                id,
                init: Some(Expr::Object(props)),
                ..
            } => {
                if boxed_vars.contains(id) || module_globals.contains_key(id) {
                    continue;
                }
                if props.is_empty() || props.len() > MAX_SCALAR_OBJECT_FIELDS {
                    continue;
                }
                // Reject method closures that need a `this` back-pointer —
                // scalar replacement can't provide one.
                let has_this_closure = props.iter().any(|(_, v)| {
                    matches!(
                        v,
                        Expr::Closure {
                            captures_this: true,
                            ..
                        }
                    )
                });
                if has_this_closure {
                    continue;
                }
                // Deduplicate keys (last-write-wins), preserve first-seen order.
                let mut keys: Vec<String> = Vec::with_capacity(props.len());
                for (k, _) in props {
                    if !keys.iter().any(|existing| existing == k) {
                        keys.push(k.clone());
                    }
                }
                candidates.insert(*id, keys);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                find_object_literal_candidates(then_branch, boxed_vars, module_globals, candidates);
                if let Some(eb) = else_branch {
                    find_object_literal_candidates(eb, boxed_vars, module_globals, candidates);
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    find_object_literal_candidates(
                        std::slice::from_ref(init_stmt),
                        boxed_vars,
                        module_globals,
                        candidates,
                    );
                }
                find_object_literal_candidates(body, boxed_vars, module_globals, candidates);
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                find_object_literal_candidates(body, boxed_vars, module_globals, candidates);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                find_object_literal_candidates(body, boxed_vars, module_globals, candidates);
                if let Some(c) = catch {
                    find_object_literal_candidates(&c.body, boxed_vars, module_globals, candidates);
                }
                if let Some(f) = finally {
                    find_object_literal_candidates(f, boxed_vars, module_globals, candidates);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    find_object_literal_candidates(&c.body, boxed_vars, module_globals, candidates);
                }
            }
            Stmt::Labeled { body, .. } => {
                find_object_literal_candidates(
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

pub fn check_object_literal_escapes_in_stmts(
    stmts: &[perry_hir::Stmt],
    candidates: &std::collections::HashMap<u32, Vec<String>>,
    escaped: &mut HashSet<u32>,
) {
    use perry_hir::Stmt;
    for s in stmts {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) => {
                check_object_literal_escapes_in_expr(e, candidates, escaped);
            }
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    check_object_literal_escapes_in_expr(e, candidates, escaped);
                }
            }
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    check_object_literal_escapes_in_expr(e, candidates, escaped);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_object_literal_escapes_in_expr(condition, candidates, escaped);
                check_object_literal_escapes_in_stmts(then_branch, candidates, escaped);
                if let Some(eb) = else_branch {
                    check_object_literal_escapes_in_stmts(eb, candidates, escaped);
                }
            }
            Stmt::While { condition, body } => {
                check_object_literal_escapes_in_expr(condition, candidates, escaped);
                check_object_literal_escapes_in_stmts(body, candidates, escaped);
            }
            Stmt::DoWhile { body, condition } => {
                check_object_literal_escapes_in_stmts(body, candidates, escaped);
                check_object_literal_escapes_in_expr(condition, candidates, escaped);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    check_object_literal_escapes_in_stmts(
                        std::slice::from_ref(init_stmt),
                        candidates,
                        escaped,
                    );
                }
                if let Some(cond) = condition {
                    check_object_literal_escapes_in_expr(cond, candidates, escaped);
                }
                if let Some(upd) = update {
                    check_object_literal_escapes_in_expr(upd, candidates, escaped);
                }
                check_object_literal_escapes_in_stmts(body, candidates, escaped);
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                check_object_literal_escapes_in_expr(discriminant, candidates, escaped);
                for case in cases {
                    if let Some(test) = &case.test {
                        check_object_literal_escapes_in_expr(test, candidates, escaped);
                    }
                    check_object_literal_escapes_in_stmts(&case.body, candidates, escaped);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                check_object_literal_escapes_in_stmts(body, candidates, escaped);
                if let Some(c) = catch {
                    check_object_literal_escapes_in_stmts(&c.body, candidates, escaped);
                }
                if let Some(f) = finally {
                    check_object_literal_escapes_in_stmts(f, candidates, escaped);
                }
            }
            Stmt::Labeled { body, .. } => {
                check_object_literal_escapes_in_stmts(
                    std::slice::from_ref(body.as_ref()),
                    candidates,
                    escaped,
                );
            }
            _ => {}
        }
    }
}

pub fn check_object_literal_escapes_in_expr(
    e: &perry_hir::Expr,
    candidates: &std::collections::HashMap<u32, Vec<String>>,
    escaped: &mut HashSet<u32>,
) {
    use perry_hir::{ArrayElement, CallArg, Expr};

    match e {
        // Safe: `o.known_field` read.
        Expr::PropertyGet { object, property } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(keys) = candidates.get(id) {
                    if keys.iter().any(|k| k == property) {
                        return;
                    }
                    // Access to a key not in the literal — would observe
                    // undefined, which we can't produce without a real object.
                    escaped.insert(*id);
                    return;
                }
            }
            check_object_literal_escapes_in_expr(object, candidates, escaped);
        }

        // Safe: `o.known_field = v` (value must not reference id).
        Expr::PropertySet { object, property, value } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(keys) = candidates.get(id) {
                    let key_known = keys.iter().any(|k| k == property);
                    if !key_known {
                        escaped.insert(*id);
                    } else if expr_contains_local_get(value, *id) {
                        escaped.insert(*id);
                    }
                    check_object_literal_escapes_in_expr(value, candidates, escaped);
                    return;
                }
            }
            check_object_literal_escapes_in_expr(object, candidates, escaped);
            check_object_literal_escapes_in_expr(value, candidates, escaped);
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
                    if let Some(keys) = candidates.get(id) {
                        let key_known = keys.iter().any(|k| k == property);
                        if !key_known {
                            escaped.insert(*id);
                        } else if expr_contains_local_get(value, *id) {
                            escaped.insert(*id);
                        }
                        check_object_literal_escapes_in_expr(value, candidates, escaped);
                        return;
                    }
                }
            }
            check_object_literal_escapes_in_expr(target, candidates, escaped);
            check_object_literal_escapes_in_expr(key, candidates, escaped);
            check_object_literal_escapes_in_expr(value, candidates, escaped);
            check_object_literal_escapes_in_expr(receiver, candidates, escaped);
        }

        // Safe: `o.known_field++`.
        Expr::PropertyUpdate { object, property, .. } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if let Some(keys) = candidates.get(id) {
                    if !keys.iter().any(|k| k == property) {
                        escaped.insert(*id);
                    }
                    return;
                }
            }
            check_object_literal_escapes_in_expr(object, candidates, escaped);
        }

        Expr::LocalSet(id, value) => {
            if candidates.contains_key(id) {
                escaped.insert(*id);
            }
            check_object_literal_escapes_in_expr(value, candidates, escaped);
        }
        Expr::LocalGet(id) => {
            if candidates.contains_key(id) {
                escaped.insert(*id);
            }
        }

        Expr::Closure { body, captures, .. } => {
            for c in captures {
                if candidates.contains_key(c) {
                    escaped.insert(*c);
                }
            }
            check_object_literal_escapes_in_stmts(body, candidates, escaped);
        }

        // ── Recurse into sub-expressions ──
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            check_object_literal_escapes_in_expr(left, candidates, escaped);
            check_object_literal_escapes_in_expr(right, candidates, escaped);
        }
        Expr::Unary { operand, .. } | Expr::Void(operand) | Expr::TypeOf(operand)
        | Expr::Await(operand) | Expr::Delete(operand)
        | Expr::StringCoerce(operand) | Expr::ObjectCoerce(operand) | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand) | Expr::IsFinite(operand)
        | Expr::IsNaN(operand) | Expr::NumberIsNaN(operand)
        | Expr::NumberIsFinite(operand) | Expr::NumberIsInteger(operand)
        | Expr::IsUndefinedOrBareNan(operand) | Expr::ParseFloat(operand) => {
            check_object_literal_escapes_in_expr(operand, candidates, escaped);
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            check_object_literal_escapes_in_expr(condition, candidates, escaped);
            check_object_literal_escapes_in_expr(then_expr, candidates, escaped);
            check_object_literal_escapes_in_expr(else_expr, candidates, escaped);
        }
        Expr::Call { callee, args, .. } => {
            // Issue #518: `cur.toArray()` where `cur` is a scalar-replaceable
            // object literal — the PropertyGet handler above treats it as
            // safe because `toArray` is a known field, but in CALLEE position
            // the receiver (`cur`) is passed implicitly to the method
            // dispatcher (`js_native_call_method` in lower_call.rs takes a
            // receiver and uses it for `this`). Without a real heap object,
            // codegen loads the dummy slot — uninitialized memory — and
            // dispatch returns NULL_OBJECT_BYTES instead of the closure's
            // result. Mark the candidate as escaped so the standard heap
            // path lowers `cur` correctly. Bare property READS (`cur.x` in
            // an arithmetic context) keep the scalar-replacement fast path.
            if let Expr::PropertyGet { object, .. } = callee.as_ref() {
                if let Expr::LocalGet(id) = object.as_ref() {
                    if candidates.contains_key(id) {
                        escaped.insert(*id);
                    }
                }
            }
            check_object_literal_escapes_in_expr(callee, candidates, escaped);
            for a in args {
                check_object_literal_escapes_in_expr(a, candidates, escaped);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            // Same #518 reasoning as `Call` above — method-call receiver
            // escapes via dispatch even though the callee form looks like a
            // safe property read.
            if let Expr::PropertyGet { object, .. } = callee.as_ref() {
                if let Expr::LocalGet(id) = object.as_ref() {
                    if candidates.contains_key(id) {
                        escaped.insert(*id);
                    }
                }
            }
            check_object_literal_escapes_in_expr(callee, candidates, escaped);
            for a in args {
                match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => {
                        check_object_literal_escapes_in_expr(e, candidates, escaped);
                    }
                }
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                check_object_literal_escapes_in_expr(o, candidates, escaped);
            }
            for a in args {
                check_object_literal_escapes_in_expr(a, candidates, escaped);
            }
        }
        Expr::IndexGet { object, index } => {
            check_object_literal_escapes_in_expr(object, candidates, escaped);
            check_object_literal_escapes_in_expr(index, candidates, escaped);
        }
        Expr::IndexSet { object, index, value } => {
            check_object_literal_escapes_in_expr(object, candidates, escaped);
            check_object_literal_escapes_in_expr(index, candidates, escaped);
            check_object_literal_escapes_in_expr(value, candidates, escaped);
        }
        Expr::Array(elements) => {
            for el in elements {
                check_object_literal_escapes_in_expr(el, candidates, escaped);
            }
        }
        Expr::ArraySpread(elements) => {
            for el in elements {
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => {
                        check_object_literal_escapes_in_expr(e, candidates, escaped);
                    }
                    ArrayElement::Hole => {}
                }
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                check_object_literal_escapes_in_expr(v, candidates, escaped);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, e) in parts {
                check_object_literal_escapes_in_expr(e, candidates, escaped);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                check_object_literal_escapes_in_expr(a, candidates, escaped);
            }
        }
        Expr::Sequence(es) => {
            for e in es {
                check_object_literal_escapes_in_expr(e, candidates, escaped);
            }
        }
        Expr::Update { id, .. } => {
            if candidates.contains_key(id) {
                escaped.insert(*id);
            }
        }
        // Known leaf variants — no sub-expressions, can't hide a LocalGet.
        Expr::Integer(_) | Expr::Number(_) | Expr::Bool(_) | Expr::String(_)
        | Expr::Undefined | Expr::Null | Expr::This | Expr::FuncRef(_)
        | Expr::ClassRef(_) | Expr::ExternFuncRef { .. } | Expr::GlobalGet(_)
        | Expr::BigInt(_)
        // Time / perf leaf intrinsics
        | Expr::DateNow | Expr::PerformanceNow | Expr::MathRandom
        | Expr::CryptoRandomUUID
        | Expr::CryptoRandomUUIDv7
        // Iter-result scratch (zero-arg leaves)
        | Expr::IterResultGetValue | Expr::IterResultGetDone
        // Process leaf intrinsics
        | Expr::ProcessCwd | Expr::ProcessUptime | Expr::ProcessArgv
        | Expr::ProcessMemoryUsage | Expr::ProcessThreadCpuUsage(_)
        | Expr::ProcessAvailableMemory | Expr::ProcessConstrainedMemory
        | Expr::ProcessPosixCredential(_)
        | Expr::ProcessCpuUsage(_)
        | Expr::ProcessResourceUsage | Expr::ProcessActiveResourcesInfo
        | Expr::ProcessPid | Expr::ProcessPpid
        | Expr::ProcessVersion | Expr::ProcessVersions | Expr::ProcessHrtimeBigint
        | Expr::ProcessHrtime(_)
        | Expr::ProcessTitle
        | Expr::ProcessStdin | Expr::ProcessStdout | Expr::ProcessStderr
        | Expr::ProcessEnv
        | Expr::GlobalThisExpr
        // Path / encoding / OS leaf intrinsics
        | Expr::PathSep | Expr::PathDelimiter
        | Expr::TextEncoderNew
        | Expr::OsPlatform | Expr::OsArch | Expr::OsHostname | Expr::OsHomedir
        | Expr::OsTmpdir | Expr::OsTotalmem | Expr::OsFreemem | Expr::OsUptime
        | Expr::OsType | Expr::OsRelease | Expr::OsCpus | Expr::OsNetworkInterfaces
        | Expr::OsUserInfo | Expr::OsUserInfoBuffer | Expr::OsEOL | Expr::OsDevNull | Expr::OsAvailableParallelism | Expr::OsEndianness | Expr::OsLoadavg | Expr::OsMachine | Expr::OsVersion
        // Collection constructors (no sub-exprs)
        | Expr::MapNew | Expr::SetNew
        // RegExp leaf accessors
        | Expr::RegExpExecIndex | Expr::RegExpExecGroups => {}
        _ => {
            // Conservative catch-all: unenumerated HIR variants may embed
            // `LocalGet(id)` references we can't see (e.g. `ProxyNew`,
            // `ObjectDefineProperty`, Reflect.* — none of which are enumerated
            // above). Mark every candidate as escaped so we don't scalar-
            // replace a local that's actually live through one of those sites.
            // The cost is losing the optimization in function bodies that use
            // exotic features; common loops stay optimized.
            for id in candidates.keys() {
                escaped.insert(*id);
            }
        }
    }
}
