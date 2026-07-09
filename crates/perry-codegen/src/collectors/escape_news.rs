use std::collections::{HashMap, HashSet};

use super::*;

pub fn collect_non_escaping_news(
    stmts: &[perry_hir::Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &std::collections::HashMap<u32, String>,
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
) -> std::collections::HashMap<u32, String> {
    // Pass 1: find candidates — Let bindings of New that aren't boxed/global.
    let mut candidates: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    find_new_candidates(stmts, boxed_vars, module_globals, &mut candidates);

    if candidates.is_empty() {
        return candidates;
    }

    // Pass 2: walk all stmts/exprs checking every use of each candidate.
    // Any unsafe use marks the id as escaped.
    let mut escaped: HashSet<u32> = HashSet::new();
    check_escapes_in_stmts(stmts, &candidates, classes, &mut escaped);

    // Pass 3 (issue #313): if the candidate's class constructor or any
    // instance-field initializer materializes `this` as a value, scalar
    // replacement cannot soundly inline it — `Expr::This` reads from the
    // dummy `this_stack` slot allocated at stmt.rs:316, which is never
    // populated (there is no real heap `this` in scalar replacement). Mark
    // such candidates as escaped so they take the heap-allocated path.
    for (id, class_name) in &candidates {
        if escaped.contains(id) {
            continue;
        }
        if let Some(class) = classes.get(class_name) {
            if class_uses_this_as_value(class, classes) {
                escaped.insert(*id);
            }
            // Issue #573: classes extending built-in Error / TypeError /
            // etc. need the heap path so `lower_new`'s Error-init fallback
            // can populate `this.message` / `this.name` via
            // `js_object_set_field_by_name`. Scalar replacement allocates
            // per-field allocas keyed by declared field names, but Error
            // subclasses typically declare neither field — the runtime
            // adds them via SuperCall / lower_new fallback. Without this
            // check, `class MyError extends Error {}` skips the heap path
            // and the scalar-replaced object has no slots for `message` /
            // `name`, so reads return undefined or crash.
            else if class_chain_extends_builtin_error(class, classes) {
                escaped.insert(*id);
            }
        }
    }

    candidates.retain(|id, _| !escaped.contains(id));
    candidates
}

/// For scalar-replaced `new` locals, collect the fields that are actually read
/// through the local after construction. This intentionally tracks only reads
/// (plus read-modify-write updates): writes still need their RHS evaluated for
/// JS side effects, but the scalar slot/store can be elided when the field is
/// never observed.
pub fn collect_non_escaping_new_used_fields(
    stmts: &[perry_hir::Stmt],
    non_escaping_news: &HashMap<u32, String>,
) -> HashMap<u32, HashSet<String>> {
    let mut used = HashMap::new();
    if non_escaping_news.is_empty() {
        return used;
    }
    collect_used_new_fields_in_stmts(stmts, non_escaping_news, &mut used);
    used
}

fn collect_used_new_fields_in_stmts(
    stmts: &[perry_hir::Stmt],
    non_escaping_news: &HashMap<u32, String>,
    used: &mut HashMap<u32, HashSet<String>>,
) {
    use perry_hir::Stmt;
    for stmt in stmts {
        match stmt {
            Stmt::Expr(e) | Stmt::Throw(e) => {
                collect_used_new_fields_in_expr(e, non_escaping_news, used)
            }
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    collect_used_new_fields_in_expr(e, non_escaping_news, used);
                }
            }
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    collect_used_new_fields_in_expr(e, non_escaping_news, used);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_used_new_fields_in_expr(condition, non_escaping_news, used);
                collect_used_new_fields_in_stmts(then_branch, non_escaping_news, used);
                if let Some(else_branch) = else_branch {
                    collect_used_new_fields_in_stmts(else_branch, non_escaping_news, used);
                }
            }
            Stmt::While { condition, body } => {
                collect_used_new_fields_in_expr(condition, non_escaping_news, used);
                collect_used_new_fields_in_stmts(body, non_escaping_news, used);
            }
            Stmt::DoWhile { body, condition } => {
                collect_used_new_fields_in_stmts(body, non_escaping_news, used);
                collect_used_new_fields_in_expr(condition, non_escaping_news, used);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    collect_used_new_fields_in_stmts(
                        std::slice::from_ref(init.as_ref()),
                        non_escaping_news,
                        used,
                    );
                }
                if let Some(condition) = condition {
                    collect_used_new_fields_in_expr(condition, non_escaping_news, used);
                }
                if let Some(update) = update {
                    collect_used_new_fields_in_expr(update, non_escaping_news, used);
                }
                collect_used_new_fields_in_stmts(body, non_escaping_news, used);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_used_new_fields_in_stmts(body, non_escaping_news, used);
                if let Some(catch) = catch {
                    collect_used_new_fields_in_stmts(&catch.body, non_escaping_news, used);
                }
                if let Some(finally) = finally {
                    collect_used_new_fields_in_stmts(finally, non_escaping_news, used);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                collect_used_new_fields_in_expr(discriminant, non_escaping_news, used);
                for case in cases {
                    if let Some(test) = &case.test {
                        collect_used_new_fields_in_expr(test, non_escaping_news, used);
                    }
                    collect_used_new_fields_in_stmts(&case.body, non_escaping_news, used);
                }
            }
            Stmt::Labeled { body, .. } => {
                collect_used_new_fields_in_stmts(
                    std::slice::from_ref(body.as_ref()),
                    non_escaping_news,
                    used,
                );
            }
            Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
            Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => {}
        }
    }
}

fn collect_used_new_fields_in_expr(
    expr: &perry_hir::Expr,
    non_escaping_news: &HashMap<u32, String>,
    used: &mut HashMap<u32, HashSet<String>>,
) {
    use perry_hir::{ArrayElement, CallArg, Expr};

    match expr {
        Expr::PropertyGet { object, property }
        | Expr::PropertyUpdate {
            object, property, ..
        } => {
            if let Expr::LocalGet(id) = object.as_ref() {
                if non_escaping_news.contains_key(id) {
                    used.entry(*id).or_default().insert(property.clone());
                    return;
                }
            }
            collect_used_new_fields_in_expr(object, non_escaping_news, used);
        }
        Expr::PropertySet { object, value, .. } => {
            collect_used_new_fields_in_expr(object, non_escaping_news, used);
            collect_used_new_fields_in_expr(value, non_escaping_news, used);
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. }
        | Expr::MathPow(left, right)
        | Expr::PathJoin(left, right)
        | Expr::PathWin32Join(left, right)
        | Expr::ObjectIs(left, right)
        | Expr::ObjectHasOwn(left, right)
        | Expr::PathRelative(left, right)
        | Expr::PathMatchesGlob(left, right)
        | Expr::PathResolveJoin(left, right)
        | Expr::FsWriteFileSync(left, right)
        | Expr::JsonParseWithReviver(left, right)
        | Expr::PathBasenameExt(left, right) => {
            collect_used_new_fields_in_expr(left, non_escaping_news, used);
            collect_used_new_fields_in_expr(right, non_escaping_news, used);
        }
        Expr::JsonParseReviver { text, reviver } => {
            collect_used_new_fields_in_expr(text, non_escaping_news, used);
            collect_used_new_fields_in_expr(reviver, non_escaping_news, used);
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
        | Expr::ArrayIsArray(operand)
        | Expr::ObjectRest {
            object: operand, ..
        }
        | Expr::SetNewFromArray(operand)
        | Expr::Atob(operand)
        | Expr::Btoa(operand) => collect_used_new_fields_in_expr(operand, non_escaping_news, used),
        Expr::JsonParseTyped { text, .. } => {
            collect_used_new_fields_in_expr(text, non_escaping_news, used)
        }
        Expr::StructuredClone { value, options } => {
            collect_used_new_fields_in_expr(value, non_escaping_news, used);
            collect_used_new_fields_in_expr(options, non_escaping_news, used);
        }
        Expr::ProcessNextTick { callback, args } => {
            collect_used_new_fields_in_expr(callback, non_escaping_news, used);
            for a in args {
                collect_used_new_fields_in_expr(a, non_escaping_news, used);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_used_new_fields_in_expr(condition, non_escaping_news, used);
            collect_used_new_fields_in_expr(then_expr, non_escaping_news, used);
            collect_used_new_fields_in_expr(else_expr, non_escaping_news, used);
        }
        Expr::Call { callee, args, .. } => {
            collect_used_new_fields_in_expr(callee, non_escaping_news, used);
            for arg in args {
                collect_used_new_fields_in_expr(arg, non_escaping_news, used);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            collect_used_new_fields_in_expr(callee, non_escaping_news, used);
            for arg in args {
                match arg {
                    CallArg::Expr(e) | CallArg::Spread(e) => {
                        collect_used_new_fields_in_expr(e, non_escaping_news, used)
                    }
                }
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(object) = object {
                collect_used_new_fields_in_expr(object, non_escaping_news, used);
            }
            for arg in args {
                collect_used_new_fields_in_expr(arg, non_escaping_news, used);
            }
        }
        Expr::IndexGet { object, index } => {
            collect_used_new_fields_in_expr(object, non_escaping_news, used);
            collect_used_new_fields_in_expr(index, non_escaping_news, used);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            collect_used_new_fields_in_expr(object, non_escaping_news, used);
            collect_used_new_fields_in_expr(index, non_escaping_news, used);
            collect_used_new_fields_in_expr(value, non_escaping_news, used);
        }
        Expr::IndexUpdate { object, index, .. } => {
            collect_used_new_fields_in_expr(object, non_escaping_news, used);
            collect_used_new_fields_in_expr(index, non_escaping_news, used);
        }
        Expr::Array(elements) => {
            for element in elements {
                collect_used_new_fields_in_expr(element, non_escaping_news, used);
            }
        }
        Expr::ArraySpread(elements) => {
            for element in elements {
                match element {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => {
                        collect_used_new_fields_in_expr(e, non_escaping_news, used)
                    }
                    ArrayElement::Hole => {}
                }
            }
        }
        Expr::Object(props) => {
            for (_, value) in props {
                collect_used_new_fields_in_expr(value, non_escaping_news, used);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, value) in parts {
                collect_used_new_fields_in_expr(value, non_escaping_news, used);
            }
        }
        Expr::New { args, .. }
        | Expr::SuperCall(args)
        | Expr::StaticMethodCall { args, .. }
        | Expr::SuperMethodCall { args, .. }
        | Expr::MathMin(args)
        | Expr::MathMax(args) => {
            for arg in args {
                collect_used_new_fields_in_expr(arg, non_escaping_news, used);
            }
        }
        Expr::ObjectSuperPropertyGet {
            home,
            key,
            receiver,
        } => {
            collect_used_new_fields_in_expr(home, non_escaping_news, used);
            collect_used_new_fields_in_expr(key, non_escaping_news, used);
            collect_used_new_fields_in_expr(receiver, non_escaping_news, used);
        }
        Expr::SuperPropertySet { key, value, .. } => {
            collect_used_new_fields_in_expr(key, non_escaping_news, used);
            collect_used_new_fields_in_expr(value, non_escaping_news, used);
        }
        Expr::ObjectSuperPropertySet {
            home,
            key,
            value,
            receiver,
        } => {
            collect_used_new_fields_in_expr(home, non_escaping_news, used);
            collect_used_new_fields_in_expr(key, non_escaping_news, used);
            collect_used_new_fields_in_expr(value, non_escaping_news, used);
            collect_used_new_fields_in_expr(receiver, non_escaping_news, used);
        }
        Expr::ObjectSuperMethodCall {
            home,
            key,
            receiver,
            args,
        } => {
            collect_used_new_fields_in_expr(home, non_escaping_news, used);
            collect_used_new_fields_in_expr(key, non_escaping_news, used);
            collect_used_new_fields_in_expr(receiver, non_escaping_news, used);
            for arg in args {
                collect_used_new_fields_in_expr(arg, non_escaping_news, used);
            }
        }
        Expr::NewDynamic { callee, args, .. } => {
            collect_used_new_fields_in_expr(callee, non_escaping_news, used);
            for arg in args {
                collect_used_new_fields_in_expr(arg, non_escaping_news, used);
            }
        }
        Expr::LocalSet(_, value) => collect_used_new_fields_in_expr(value, non_escaping_news, used),
        Expr::Sequence(values) => {
            for value in values {
                collect_used_new_fields_in_expr(value, non_escaping_news, used);
            }
        }
        Expr::Closure { body, .. } => {
            collect_used_new_fields_in_stmts(body, non_escaping_news, used);
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
            collect_used_new_fields_in_expr(array, non_escaping_news, used);
            collect_used_new_fields_in_expr(callback, non_escaping_news, used);
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
            collect_used_new_fields_in_expr(array, non_escaping_news, used);
            collect_used_new_fields_in_expr(callback, non_escaping_news, used);
            if let Some(initial) = initial {
                collect_used_new_fields_in_expr(initial, non_escaping_news, used);
            }
        }
        Expr::ArrayPush { value, .. }
        | Expr::ArrayUnshift { value, .. }
        | Expr::SetAdd { value, .. }
        | Expr::StaticFieldSet { value, .. } => {
            collect_used_new_fields_in_expr(value, non_escaping_news, used)
        }
        Expr::ArraySplice {
            start,
            delete_count,
            items,
            ..
        } => {
            collect_used_new_fields_in_expr(start, non_escaping_news, used);
            if let Some(delete_count) = delete_count {
                collect_used_new_fields_in_expr(delete_count, non_escaping_news, used);
            }
            for item in items {
                collect_used_new_fields_in_expr(item, non_escaping_news, used);
            }
        }
        Expr::MapSet { map, key, value } => {
            collect_used_new_fields_in_expr(map, non_escaping_news, used);
            collect_used_new_fields_in_expr(key, non_escaping_news, used);
            collect_used_new_fields_in_expr(value, non_escaping_news, used);
        }
        Expr::MapGet { map, key } | Expr::MapHas { map, key } | Expr::MapDelete { map, key } => {
            collect_used_new_fields_in_expr(map, non_escaping_news, used);
            collect_used_new_fields_in_expr(key, non_escaping_news, used);
        }
        Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
            collect_used_new_fields_in_expr(set, non_escaping_news, used);
            collect_used_new_fields_in_expr(value, non_escaping_news, used);
        }
        Expr::ErrorNew(opt) | Expr::Yield { value: opt, .. } => {
            if let Some(value) = opt {
                collect_used_new_fields_in_expr(value, non_escaping_news, used);
            }
        }
        Expr::ArrayJoin { array, separator } => {
            collect_used_new_fields_in_expr(array, non_escaping_news, used);
            if let Some(separator) = separator {
                collect_used_new_fields_in_expr(separator, non_escaping_news, used);
            }
        }
        Expr::ArraySlice { array, start, end } => {
            collect_used_new_fields_in_expr(array, non_escaping_news, used);
            collect_used_new_fields_in_expr(start, non_escaping_news, used);
            if let Some(end) = end {
                collect_used_new_fields_in_expr(end, non_escaping_news, used);
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
            collect_used_new_fields_in_expr(array, non_escaping_news, used);
            collect_used_new_fields_in_expr(value, non_escaping_news, used);
            if let Some(fi) = from_index {
                collect_used_new_fields_in_expr(fi, non_escaping_news, used);
            }
        }
        Expr::I18nString { params, .. } => {
            for (_, value) in params {
                collect_used_new_fields_in_expr(value, non_escaping_news, used);
            }
        }
        Expr::ParseInt { string, radix } => {
            collect_used_new_fields_in_expr(string, non_escaping_news, used);
            if let Some(radix) = radix {
                collect_used_new_fields_in_expr(radix, non_escaping_news, used);
            }
        }
        Expr::JsonStringifyFull(value, replacer, indent) => {
            collect_used_new_fields_in_expr(value, non_escaping_news, used);
            collect_used_new_fields_in_expr(replacer, non_escaping_news, used);
            collect_used_new_fields_in_expr(indent, non_escaping_news, used);
        }
        Expr::RegExpTest { regex, string } | Expr::RegExpExec { regex, string } => {
            collect_used_new_fields_in_expr(regex, non_escaping_news, used);
            collect_used_new_fields_in_expr(string, non_escaping_news, used);
        }
        Expr::In { property, object } => {
            collect_used_new_fields_in_expr(property, non_escaping_news, used);
            collect_used_new_fields_in_expr(object, non_escaping_news, used);
        }
        Expr::InstanceOf { expr, .. } => {
            collect_used_new_fields_in_expr(expr, non_escaping_news, used);
        }
        Expr::ProcessOn { event, handler } => {
            collect_used_new_fields_in_expr(event, non_escaping_news, used);
            collect_used_new_fields_in_expr(handler, non_escaping_news, used);
        }
        Expr::FinalizationRegistryRegister {
            registry,
            target,
            held,
            token,
        } => {
            collect_used_new_fields_in_expr(registry, non_escaping_news, used);
            collect_used_new_fields_in_expr(target, non_escaping_news, used);
            collect_used_new_fields_in_expr(held, non_escaping_news, used);
            if let Some(token) = token {
                collect_used_new_fields_in_expr(token, non_escaping_news, used);
            }
        }
        Expr::FinalizationRegistryUnregister { registry, token } => {
            collect_used_new_fields_in_expr(registry, non_escaping_news, used);
            collect_used_new_fields_in_expr(token, non_escaping_news, used);
        }
        Expr::ArrayFromMapped {
            iterable,
            map_fn,
            this_arg,
        } => {
            collect_used_new_fields_in_expr(iterable, non_escaping_news, used);
            collect_used_new_fields_in_expr(map_fn, non_escaping_news, used);
            if let Some(t) = this_arg {
                collect_used_new_fields_in_expr(t, non_escaping_news, used);
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
            collect_used_new_fields_in_expr(iterable, non_escaping_news, used);
            collect_used_new_fields_in_expr(map_fn, non_escaping_news, used);
        }
        Expr::ArrayToSorted { array, comparator } => {
            collect_used_new_fields_in_expr(array, non_escaping_news, used);
            if let Some(comparator) = comparator {
                collect_used_new_fields_in_expr(comparator, non_escaping_news, used);
            }
        }
        Expr::ArrayToReversed { array }
        | Expr::ArrayReverseValue { receiver: array }
        | Expr::ArrayFlat { array }
        | Expr::ArrayEntries(array)
        | Expr::ArrayKeys(array)
        | Expr::ArrayValues(array) => {
            collect_used_new_fields_in_expr(array, non_escaping_news, used);
        }
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => {
            collect_used_new_fields_in_expr(array, non_escaping_news, used);
            collect_used_new_fields_in_expr(start, non_escaping_news, used);
            collect_used_new_fields_in_expr(delete_count, non_escaping_news, used);
            for item in items {
                collect_used_new_fields_in_expr(item, non_escaping_news, used);
            }
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            collect_used_new_fields_in_expr(array, non_escaping_news, used);
            collect_used_new_fields_in_expr(index, non_escaping_news, used);
            collect_used_new_fields_in_expr(value, non_escaping_news, used);
        }
        Expr::ArrayCopyWithin {
            target, start, end, ..
        } => {
            collect_used_new_fields_in_expr(target, non_escaping_news, used);
            collect_used_new_fields_in_expr(start, non_escaping_news, used);
            if let Some(end) = end {
                collect_used_new_fields_in_expr(end, non_escaping_news, used);
            }
        }
        Expr::ArrayCopyWithinValue {
            receiver,
            target,
            start,
            end,
        } => {
            collect_used_new_fields_in_expr(receiver, non_escaping_news, used);
            collect_used_new_fields_in_expr(target, non_escaping_news, used);
            collect_used_new_fields_in_expr(start, non_escaping_news, used);
            if let Some(end) = end {
                collect_used_new_fields_in_expr(end, non_escaping_news, used);
            }
        }
        Expr::ArrayAt { array, index } => {
            collect_used_new_fields_in_expr(array, non_escaping_news, used);
            collect_used_new_fields_in_expr(index, non_escaping_news, used);
        }
        Expr::TypedArrayNew { arg, .. } => {
            if let Some(arg) = arg {
                collect_used_new_fields_in_expr(arg, non_escaping_news, used);
            }
        }
        Expr::ChildProcessExecSync { command, options } => {
            collect_used_new_fields_in_expr(command, non_escaping_news, used);
            if let Some(options) = options {
                collect_used_new_fields_in_expr(options, non_escaping_news, used);
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
            collect_used_new_fields_in_expr(command, non_escaping_news, used);
            if let Some(args) = args {
                collect_used_new_fields_in_expr(args, non_escaping_news, used);
            }
            if let Some(options) = options {
                collect_used_new_fields_in_expr(options, non_escaping_news, used);
            }
        }
        Expr::ChildProcessExec {
            command,
            options,
            callback,
        } => {
            collect_used_new_fields_in_expr(command, non_escaping_news, used);
            if let Some(options) = options {
                collect_used_new_fields_in_expr(options, non_escaping_news, used);
            }
            if let Some(callback) = callback {
                collect_used_new_fields_in_expr(callback, non_escaping_news, used);
            }
        }
        Expr::ChildProcessExecFile {
            file,
            args,
            options,
            callback,
        } => {
            collect_used_new_fields_in_expr(file, non_escaping_news, used);
            if let Some(args) = args {
                collect_used_new_fields_in_expr(args, non_escaping_news, used);
            }
            if let Some(options) = options {
                collect_used_new_fields_in_expr(options, non_escaping_news, used);
            }
            if let Some(callback) = callback {
                collect_used_new_fields_in_expr(callback, non_escaping_news, used);
            }
        }
        Expr::ChildProcessExecFileSync {
            file,
            args,
            options,
        } => {
            collect_used_new_fields_in_expr(file, non_escaping_news, used);
            if let Some(args) = args {
                collect_used_new_fields_in_expr(args, non_escaping_news, used);
            }
            if let Some(options) = options {
                collect_used_new_fields_in_expr(options, non_escaping_news, used);
            }
        }
        Expr::ChildProcessSpawnBackground {
            command,
            args,
            log_file,
            env_json,
        } => {
            collect_used_new_fields_in_expr(command, non_escaping_news, used);
            if let Some(args) = args {
                collect_used_new_fields_in_expr(args, non_escaping_news, used);
            }
            collect_used_new_fields_in_expr(log_file, non_escaping_news, used);
            if let Some(env_json) = env_json {
                collect_used_new_fields_in_expr(env_json, non_escaping_news, used);
            }
        }
        Expr::ChildProcessGetProcessStatus(handle) | Expr::ChildProcessKillProcess(handle) => {
            collect_used_new_fields_in_expr(handle, non_escaping_news, used);
        }
        Expr::FetchWithOptions {
            url,
            method,
            body,
            headers,
            headers_dynamic,
            signal,
        } => {
            collect_used_new_fields_in_expr(url, non_escaping_news, used);
            collect_used_new_fields_in_expr(method, non_escaping_news, used);
            collect_used_new_fields_in_expr(body, non_escaping_news, used);
            for (_, value) in headers {
                collect_used_new_fields_in_expr(value, non_escaping_news, used);
            }
            if let Some(hd) = headers_dynamic {
                collect_used_new_fields_in_expr(hd, non_escaping_news, used);
            }
            if let Some(s) = signal {
                collect_used_new_fields_in_expr(s, non_escaping_news, used);
            }
        }
        Expr::FetchGetWithAuth { url, auth_header } => {
            collect_used_new_fields_in_expr(url, non_escaping_news, used);
            collect_used_new_fields_in_expr(auth_header, non_escaping_news, used);
        }
        Expr::FetchPostWithAuth {
            url,
            auth_header,
            body,
        } => {
            collect_used_new_fields_in_expr(url, non_escaping_news, used);
            collect_used_new_fields_in_expr(auth_header, non_escaping_news, used);
            collect_used_new_fields_in_expr(body, non_escaping_news, used);
        }
        Expr::JsonStringifyPretty {
            value,
            replacer,
            space,
        } => {
            collect_used_new_fields_in_expr(value, non_escaping_news, used);
            if let Some(replacer) = replacer {
                collect_used_new_fields_in_expr(replacer, non_escaping_news, used);
            }
            collect_used_new_fields_in_expr(space, non_escaping_news, used);
        }
        Expr::Integer(_)
        | Expr::Number(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Undefined
        | Expr::Null
        | Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::This
        | Expr::FuncRef(_)
        | Expr::ClassRef(_)
        | Expr::ExternFuncRef { .. }
        | Expr::DateNow
        | Expr::PerformanceNow
        | Expr::MapNew
        | Expr::SetNew
        | Expr::EnumMember { .. }
        | Expr::StaticFieldGet { .. }
        | Expr::RegExp { .. }
        | Expr::Uint8ArrayNew(None)
        | Expr::ArrayPop(_)
        | Expr::ArrayShift(_)
        | Expr::Update { .. }
        | Expr::BigInt(_) => {}
        // Any new Expr variant that contains a `new T(...)` allocation
        // must be listed explicitly above so scalar replacement can see
        // its used fields. Unhandled variants fall through here safely
        // (the pass remains correct, just conservative).
        _ => {}
    }
}

// ── Escape analysis for scalar replacement of non-escaping array literals ──

/// Upper bound on array length for scalar replacement. Larger literals pay
/// per-element alloca + store even when every slot is dead, and the gain over
/// the exact-sized arena allocator shrinks as N grows. 16 matches the old
/// `MIN_ARRAY_CAPACITY` ceiling so we cover every size the previous allocator
/// would have padded anyway.
pub(crate) const MAX_SCALAR_ARRAY_LEN: usize = 16;
