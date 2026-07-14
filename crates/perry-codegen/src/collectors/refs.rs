use perry_hir::WithSetFallback;
use std::collections::HashSet;

pub fn collect_let_ids(stmts: &[perry_hir::Stmt], out: &mut HashSet<u32>) {
    for s in stmts {
        match s {
            perry_hir::Stmt::Let { id, .. } => {
                out.insert(*id);
            }
            perry_hir::Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_let_ids(then_branch, out);
                if let Some(eb) = else_branch {
                    collect_let_ids(eb, out);
                }
            }
            perry_hir::Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_let_ids(std::slice::from_ref(init_stmt), out);
                }
                collect_let_ids(body, out);
            }
            perry_hir::Stmt::While { body, .. } | perry_hir::Stmt::DoWhile { body, .. } => {
                collect_let_ids(body, out);
            }
            // Try/Switch/Labeled: lets nested under these constructs were
            // previously invisible to the boxing analysis' `declared` set.
            // A `let x = …` inside `try { let x = …; … }` would not be
            // included in the per-scope declared set, so even if the rest
            // of the analysis recognised x as captured-and-mutated, the
            // box was never allocated.
            perry_hir::Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_let_ids(body, out);
                if let Some(c) = catch {
                    if let Some((id, _)) = c.param {
                        out.insert(id);
                    }
                    collect_let_ids(&c.body, out);
                }
                if let Some(f) = finally {
                    collect_let_ids(f, out);
                }
            }
            perry_hir::Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_let_ids(&case.body, out);
                }
            }
            perry_hir::Stmt::Labeled { body, .. } => {
                collect_let_ids(std::slice::from_ref(body.as_ref()), out);
            }
            _ => {}
        }
    }
}

/// Walk a sequence of statements and collect all LocalIds referenced via
/// `LocalGet`, `LocalSet`, or `Update`. Used together with `collect_let_ids`
/// to detect references to module-level lets that need globalization.
pub fn collect_ref_ids_in_stmts(stmts: &[perry_hir::Stmt], out: &mut HashSet<u32>) {
    for s in stmts {
        match s {
            perry_hir::Stmt::Expr(e) | perry_hir::Stmt::Throw(e) => collect_ref_ids_in_expr(e, out),
            perry_hir::Stmt::Return(opt) => {
                if let Some(e) = opt {
                    collect_ref_ids_in_expr(e, out);
                }
            }
            perry_hir::Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    collect_ref_ids_in_expr(e, out);
                }
            }
            perry_hir::Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_ref_ids_in_expr(condition, out);
                collect_ref_ids_in_stmts(then_branch, out);
                if let Some(eb) = else_branch {
                    collect_ref_ids_in_stmts(eb, out);
                }
            }
            perry_hir::Stmt::While { condition, body } => {
                collect_ref_ids_in_expr(condition, out);
                collect_ref_ids_in_stmts(body, out);
            }
            perry_hir::Stmt::DoWhile { body, condition } => {
                collect_ref_ids_in_stmts(body, out);
                collect_ref_ids_in_expr(condition, out);
            }
            perry_hir::Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init_stmt) = init {
                    collect_ref_ids_in_stmts(std::slice::from_ref(init_stmt), out);
                }
                if let Some(cond) = condition {
                    collect_ref_ids_in_expr(cond, out);
                }
                if let Some(upd) = update {
                    collect_ref_ids_in_expr(upd, out);
                }
                collect_ref_ids_in_stmts(body, out);
            }
            // Try/Switch/Labeled: previously fell through to `_ => {}` and
            // every LocalGet/LocalSet inside their body was invisible to the
            // boxing analysis. For the self-recursive-closure path
            // (`let dispatch = (i) => { try { dispatch(i+1); } catch (e) {} }`),
            // skipping the try body meant `closure_refs.contains(dispatch_id)`
            // was false, dispatch wasn't recognized as self-recursive, and
            // capture[0] held the pre-let `undefined` value. The recursive
            // call invoked an undefined closure, the body never ran, and the
            // function returned `undefined`. Hono's compose dispatches every
            // middleware through this exact shape (`try { res = await
            // handler(c, () => dispatch(i+1)); } catch (err) {…}`).
            perry_hir::Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_ref_ids_in_stmts(body, out);
                if let Some(c) = catch {
                    if let Some((id, _)) = c.param {
                        out.insert(id);
                    }
                    collect_ref_ids_in_stmts(&c.body, out);
                }
                if let Some(f) = finally {
                    collect_ref_ids_in_stmts(f, out);
                }
            }
            perry_hir::Stmt::Switch {
                discriminant,
                cases,
            } => {
                collect_ref_ids_in_expr(discriminant, out);
                for case in cases {
                    if let Some(t) = &case.test {
                        collect_ref_ids_in_expr(t, out);
                    }
                    collect_ref_ids_in_stmts(&case.body, out);
                }
            }
            perry_hir::Stmt::Labeled { body, .. } => {
                collect_ref_ids_in_stmts(std::slice::from_ref(body.as_ref()), out);
            }
            _ => {}
        }
    }
}

pub fn collect_ref_ids_in_expr(e: &perry_hir::Expr, out: &mut HashSet<u32>) {
    use perry_hir::{ArrayElement, CallArg, Expr};
    let walk = |sub: &Expr, out: &mut HashSet<u32>| {
        collect_ref_ids_in_expr(sub, out);
    };
    match e {
        Expr::LocalGet(id) => {
            out.insert(*id);
        }
        Expr::LocalSet(id, value) => {
            out.insert(*id);
            walk(value, out);
        }
        Expr::WithGet {
            object, fallback, ..
        } => {
            walk(object, out);
            walk(fallback, out);
        }
        Expr::WithSet {
            object,
            value,
            fallback,
            ..
        } => {
            walk(object, out);
            walk(value, out);
            if let WithSetFallback::Local(id) | WithSetFallback::SloppyImplicit(id) = fallback {
                out.insert(*id);
            }
        }
        Expr::Update { id, .. } => {
            out.insert(*id);
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            walk(left, out);
            walk(right, out);
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
        | Expr::ObjectFromEntries(operand)
        | Expr::ObjectIsFrozen(operand)
        | Expr::ObjectIsSealed(operand)
        | Expr::ObjectIsExtensible(operand)
        | Expr::ReflectIsExtensible(operand)
        | Expr::ReflectPreventExtensions(operand)
        | Expr::SetSize(operand)
        | Expr::SetClear(operand)
        | Expr::ArrayFrom(operand)
        | Expr::ArrayFromArrayLikeHoley(operand)
        | Expr::IteratorFrom(operand)
        | Expr::Uint8ArrayFrom(operand)
        | Expr::IteratorToArray(operand)
        | Expr::GetIterator(operand)
        | Expr::GetAsyncIterator(operand)
        | Expr::ForOfToArray(operand)
        | Expr::ForAwaitToArray(operand)
        | Expr::WeakRefNew(operand)
        | Expr::WeakRefDeref(operand)
        | Expr::QueueMicrotask(operand)
        | Expr::FsExistsSync(operand)
        | Expr::FsReadFileSync(operand)
        | Expr::FsReadFileBinary(operand)
        | Expr::FsUnlinkSync(operand)
        | Expr::FsMkdirSync(operand)
        | Expr::PathDirname(operand)
        | Expr::PathBasename(operand)
        | Expr::PathExtname(operand)
        | Expr::PathResolve(operand)
        | Expr::PathNormalize(operand)
        | Expr::PathFormat(operand)
        | Expr::PathParse(operand)
        | Expr::PathToNamespacedPath(operand)
        | Expr::DateToISOString(operand)
        | Expr::DateParse(operand)
        | Expr::EnvGetDynamic(operand)
        | Expr::ErrorNew(Some(operand))
        | Expr::FinalizationRegistryNew(operand)
        | Expr::Uint8ArrayNew(Some(operand))
        | Expr::Uint8ArrayLength(operand)
        | Expr::JsonParse(operand)
        | Expr::JsonRawJson(operand)
        | Expr::JsonIsRawJson(operand)
        | Expr::MathSqrt(operand)
        | Expr::MathFloor(operand)
        | Expr::MathCeil(operand)
        | Expr::MathRound(operand)
        | Expr::MathTrunc(operand)
        | Expr::MathSign(operand)
        | Expr::MathAbs(operand)
        | Expr::MathLog(operand)
        | Expr::MathLog2(operand)
        | Expr::MathLog10(operand)
        | Expr::MathLog1p(operand)
        | Expr::MathClz32(operand)
        | Expr::MathF16round(operand)
        | Expr::MathMinSpread(operand)
        | Expr::MathMaxSpread(operand) => {
            walk(operand, out);
        }
        Expr::StructuredClone { value, options } => {
            walk(value, out);
            walk(options, out);
        }
        Expr::ObjectCreate(proto, props) => {
            walk(proto, out);
            if let Some(props) = props {
                walk(props, out);
            }
        }
        Expr::JsonParseTyped { text, .. } => walk(text, out),
        Expr::ProcessNextTick { callback, args } => {
            walk(callback, out);
            for a in args {
                walk(a, out);
            }
        }
        Expr::Call { callee, args, .. } => {
            walk(callee, out);
            for a in args {
                walk(a, out);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            walk(callee, out);
            for a in args {
                match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => walk(e, out),
                }
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                walk(o, out);
            }
            for a in args {
                walk(a, out);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            walk(condition, out);
            walk(then_expr, out);
            walk(else_expr, out);
        }
        Expr::PropertyGet { object, .. } => walk(object, out),
        Expr::PropertySet { object, value, .. } => {
            walk(object, out);
            walk(value, out);
        }
        Expr::PropertyUpdate { object, .. } => walk(object, out),
        Expr::IndexGet { object, index } => {
            walk(object, out);
            walk(index, out);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            walk(object, out);
            walk(index, out);
            walk(value, out);
        }
        Expr::ArrayPush { array_id, value } => {
            out.insert(*array_id);
            walk(value, out);
        }
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
            out.insert(*id);
        }
        Expr::ArraySplice {
            array_id,
            start,
            delete_count,
            items,
        } => {
            out.insert(*array_id);
            walk(start, out);
            if let Some(d) = delete_count {
                walk(d, out);
            }
            for it in items {
                walk(it, out);
            }
        }
        Expr::Array(elements) => {
            for el in elements {
                walk(el, out);
            }
        }
        Expr::ArraySpread(elements) => {
            for el in elements {
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => walk(e, out),
                    ArrayElement::Hole => {}
                }
            }
        }
        Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArraySort {
            array,
            comparator: callback,
        }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArrayFindLast { array, callback }
        | Expr::ArrayFindLastIndex { array, callback }
        | Expr::ArrayForEach { array, callback }
        | Expr::ArrayFlatMap { array, callback } => {
            walk(array, out);
            walk(callback, out);
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
            walk(array, out);
            walk(callback, out);
            if let Some(init) = initial {
                walk(init, out);
            }
        }
        Expr::ArrayJoin { array, separator } => {
            walk(array, out);
            if let Some(sep) = separator {
                walk(sep, out);
            }
        }
        Expr::ArraySlice { array, start, end } => {
            walk(array, out);
            walk(start, out);
            if let Some(e) = end {
                walk(e, out);
            }
        }
        Expr::ArrayIncludes {
            array,
            value,
            from_index,
        } => {
            walk(array, out);
            walk(value, out);
            if let Some(fi) = from_index {
                walk(fi, out);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                walk(v, out);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, e) in parts {
                walk(e, out);
            }
        }
        Expr::ObjectRest { object, .. } => walk(object, out),
        Expr::ObjectIs(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::ObjectHasOwn(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::New { args, .. } => {
            for a in args {
                walk(a, out);
            }
        }
        // Issue #838 followup (b): without this arm
        // `collect_ref_ids_in_expr`'s catch-all silently skipped the
        // dynamic-callee + arg subtree, so a module-level `var _ = …;
        // var O = function(){ return new _(t); }` left `_` out of
        // `referenced_from_fn` and the module-globalisation pass
        // skipped it. O's body then read `LocalGet(_)` through the
        // soft fallback (returns 0.0) and `js_new_function_construct`
        // saw a NaN-zero callee, allocating a class_id=0 empty object
        // with no prototype-method dispatch.
        Expr::NewDynamic { callee, args, .. } => {
            walk(callee, out);
            for a in args {
                walk(a, out);
            }
        }
        // Same gap for `Expr::RegisterFunctionPrototypeMethod` (#838
        // followup (b)): the recogniser routes `Foo.prototype.x = fn`
        // (and `var p = Foo.prototype; p.x = fn`) through this node
        // with the function ref as a subtree. If the function ref is
        // a module-level local and the surrounding context is a
        // nested closure, the catch-all dropped the ref and the
        // globaliser skipped the local — the runtime helper then saw
        // a NaN-zero callee.
        Expr::RegisterFunctionPrototypeMethod { func, value, .. } => {
            walk(func, out);
            walk(value, out);
        }
        Expr::GetFunctionPrototypeMethod { func, .. } => {
            walk(func, out);
        }
        Expr::MapNew | Expr::SetNew => {}
        Expr::SetNewFromArray(arr) => walk(arr, out),
        Expr::MapSet { map, key, value } => {
            walk(map, out);
            walk(key, out);
            walk(value, out);
        }
        Expr::MapGet { map, key } | Expr::MapHas { map, key } | Expr::MapDelete { map, key } => {
            walk(map, out);
            walk(key, out);
        }
        Expr::MapClear(map) => walk(map, out),
        Expr::SetAdd { set_id, value } => {
            out.insert(*set_id);
            walk(value, out);
        }
        Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
            walk(set, out);
            walk(value, out);
        }
        Expr::MathMin(values) | Expr::MathMax(values) => {
            for v in values {
                walk(v, out);
            }
        }
        Expr::MathPow(a, b)
        | Expr::PathJoin(a, b)
        | Expr::PathRelative(a, b)
        | Expr::PathWin32Join(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::PathBasenameExt(a, b) | Expr::PathMatchesGlob(a, b) | Expr::PathResolveJoin(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::PathWin32 { args, .. } => {
            for v in args {
                walk(v, out);
            }
        }
        Expr::JsonStringifyFull(value, replacer, indent) => {
            walk(value, out);
            walk(replacer, out);
            walk(indent, out);
        }
        Expr::JsonParseReviver { text, reviver } => {
            walk(text, out);
            walk(reviver, out);
        }
        Expr::JsonParseWithReviver(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        Expr::ReflectDefineMetadata {
            key,
            value,
            target,
            property_key,
        } => {
            walk(key, out);
            walk(value, out);
            walk(target, out);
            if let Some(property_key) = property_key {
                walk(property_key, out);
            }
        }
        Expr::ReflectGetMetadata {
            key,
            target,
            property_key,
        }
        | Expr::ReflectGetOwnMetadata {
            key,
            target,
            property_key,
        }
        | Expr::ReflectHasMetadata {
            key,
            target,
            property_key,
        }
        | Expr::ReflectHasOwnMetadata {
            key,
            target,
            property_key,
        }
        | Expr::ReflectDeleteMetadata {
            key,
            target,
            property_key,
        } => {
            walk(key, out);
            walk(target, out);
            if let Some(property_key) = property_key {
                walk(property_key, out);
            }
        }
        Expr::ReflectGetMetadataKeys {
            target,
            property_key,
        }
        | Expr::ReflectGetOwnMetadataKeys {
            target,
            property_key,
        } => {
            walk(target, out);
            if let Some(property_key) = property_key {
                walk(property_key, out);
            }
        }
        Expr::Closure { body, captures, .. } => {
            // Closure literals don't introduce captures into the outer
            // scope, but their explicit captures + body references may
            // mention outer locals that need to be globalized.
            for c in captures {
                out.insert(*c);
            }
            collect_ref_ids_in_stmts(body, out);
        }
        Expr::ParseInt { string, radix } => {
            walk(string, out);
            if let Some(r) = radix {
                walk(r, out);
            }
        }
        Expr::Sequence(es) => {
            for e in es {
                walk(e, out);
            }
        }
        Expr::InstanceOf { expr, ty_expr, .. } => {
            walk(expr, out);
            if let Some(t) = ty_expr {
                walk(t, out);
            }
        }
        Expr::In { property, object } => {
            walk(property, out);
            walk(object, out);
        }
        Expr::SuperCall(args)
        | Expr::SuperMethodCall { args, .. }
        | Expr::StaticMethodCall { args, .. } => {
            for a in args {
                walk(a, out);
            }
        }
        Expr::ObjectSuperPropertyGet {
            home,
            key,
            receiver,
        } => {
            walk(home, out);
            walk(key, out);
            walk(receiver, out);
        }
        Expr::SuperPropertySet { key, value, .. } => {
            walk(key, out);
            walk(value, out);
        }
        Expr::ObjectSuperPropertySet {
            home,
            key,
            value,
            receiver,
        } => {
            walk(home, out);
            walk(key, out);
            walk(value, out);
            walk(receiver, out);
        }
        Expr::ObjectSuperMethodCall {
            home,
            key,
            receiver,
            args,
        } => {
            walk(home, out);
            walk(key, out);
            walk(receiver, out);
            for a in args {
                walk(a, out);
            }
        }
        Expr::FsWriteFileSync(p, c) => {
            walk(p, out);
            walk(c, out);
        }
        Expr::ErrorNewWithCause { message, cause } => {
            walk(message, out);
            walk(cause, out);
        }
        Expr::ErrorNewWithOptions {
            message, options, ..
        } => {
            walk(message, out);
            walk(options, out);
        }
        Expr::DateNew(args) => {
            for a in args {
                walk(a, out);
            }
        }
        Expr::Uint8ArrayGet { array, index } => {
            walk(array, out);
            walk(index, out);
        }
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => {
            walk(array, out);
            walk(index, out);
            walk(value, out);
        }
        Expr::TypedArrayNew { arg, .. } => {
            if let Some(a) = arg {
                walk(a, out);
            }
        }
        Expr::ObjectGroupBy { items, key_fn } | Expr::MapGroupBy { items, key_fn } => {
            walk(items, out);
            walk(key_fn, out);
        }
        Expr::ArrayFromMapped {
            iterable,
            map_fn,
            this_arg,
        } => {
            walk(iterable, out);
            walk(map_fn, out);
            if let Some(t) = this_arg {
                walk(t, out);
            }
        }
        Expr::RegExpTest { regex, string } | Expr::RegExpExec { regex, string } => {
            walk(regex, out);
            walk(string, out);
        }
        Expr::StringMatch { string, regex } => {
            walk(string, out);
            walk(regex, out);
        }
        Expr::BufferFrom { data, encoding } => {
            walk(data, out);
            if let Some(e) = encoding {
                walk(e, out);
            }
        }
        Expr::BufferFromArrayBuffer {
            data,
            byte_offset,
            length,
        } => {
            walk(data, out);
            walk(byte_offset, out);
            if let Some(e) = length {
                walk(e, out);
            }
        }
        Expr::BufferAlloc {
            size,
            fill,
            encoding,
        } => {
            walk(size, out);
            if let Some(f) = fill {
                walk(f, out);
            }
            if let Some(e) = encoding {
                walk(e, out);
            }
        }
        Expr::FinalizationRegistryRegister {
            registry,
            target,
            held,
            token,
        } => {
            walk(registry, out);
            walk(target, out);
            walk(held, out);
            if let Some(t) = token {
                walk(t, out);
            }
        }
        Expr::FinalizationRegistryUnregister { registry, token } => {
            walk(registry, out);
            walk(token, out);
        }
        Expr::StaticFieldSet { value, .. } => walk(value, out),
        // Array methods that aren't covered by the operand-list groups above.
        // Without these arms the catch-all `_ => {}` returns no refs, so the
        // array escape analysis mis-classifies `let arr = [...]; arr.at(i)` /
        // `arr.entries()` / `arr.values()` etc. as non-escaping and scalar-
        // replaces the literal into per-element allocas. Subsequent
        // `js_array_at(NULL, i)` then reads garbage and returns undefined
        // (issue #91 follow-up: gap test_gap_array_methods regression).
        Expr::ArrayAt { array, index } => {
            walk(array, out);
            walk(index, out);
        }
        Expr::ArrayEntries(array)
        | Expr::ArrayKeys(array)
        | Expr::ArrayValues(array)
        | Expr::ArrayFlat { array }
        | Expr::ArrayToReversed { array }
        | Expr::ArrayReverseValue { receiver: array } => {
            walk(array, out);
        }
        Expr::ArrayUnshift { array_id, value } => {
            out.insert(*array_id);
            walk(value, out);
        }
        Expr::ArrayPushSpread { array_id, source } => {
            out.insert(*array_id);
            walk(source, out);
        }
        Expr::ArrayIndexOf {
            array,
            value,
            from_index,
        }
        | Expr::ArrayLastIndexOf {
            array,
            value,
            from_index,
        } => {
            walk(array, out);
            walk(value, out);
            if let Some(fi) = from_index {
                walk(fi, out);
            }
        }
        Expr::ArraySome { array, callback } | Expr::ArrayEvery { array, callback } => {
            walk(array, out);
            walk(callback, out);
        }
        Expr::ArrayToSorted { array, comparator } => {
            walk(array, out);
            if let Some(c) = comparator {
                walk(c, out);
            }
        }
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => {
            walk(array, out);
            walk(start, out);
            walk(delete_count, out);
            for it in items {
                walk(it, out);
            }
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            walk(array, out);
            walk(index, out);
            walk(value, out);
        }
        Expr::ArrayCopyWithin {
            array_id,
            target,
            start,
            end,
        } => {
            out.insert(*array_id);
            walk(target, out);
            walk(start, out);
            if let Some(e) = end {
                walk(e, out);
            }
        }
        Expr::ArrayCopyWithinValue {
            receiver,
            target,
            start,
            end,
        } => {
            walk(receiver, out);
            walk(target, out);
            walk(start, out);
            if let Some(e) = end {
                walk(e, out);
            }
        }
        // Issue #894: prepended to the Sequence wrapping `Expr::ClassRef`
        // for class expressions returned from factory functions. The
        // key/value reference module-level (or function-local) lets;
        // collecting their refs here is what causes the surrounding
        // function-body scan to add those lets to `referenced_from_fn`,
        // which is what `module_globals` is built from.
        Expr::RegisterClassStaticSymbol {
            key_expr,
            value_expr,
            ..
        } => {
            walk(key_expr, out);
            walk(value_expr, out);
        }
        Expr::RegisterClassComputedMethod { key_expr, .. }
        | Expr::RegisterClassComputedAccessor { key_expr, .. } => {
            walk(key_expr, out);
        }
        Expr::ClassExprFresh {
            named_statics,
            symbol_statics,
            captured_args,
            ..
        } => {
            for (_, v) in named_statics {
                walk(v, out);
            }
            for (k, v) in symbol_statics {
                walk(k, out);
                walk(v, out);
            }
            for a in captured_args {
                walk(a, out);
            }
        }
        Expr::RegisterClassParentDynamic { parent_expr, .. } => {
            walk(parent_expr, out);
        }
        // Issue #859: any Expr variant not explicitly listed above descends
        // through the centralized walker. The pre-fix `_ => {}` catch-all
        // silently dropped child Exprs of newly-added variants — most
        // damagingly the async-to-generator transform's `IterResultSet(
        // value, done)` / `AsyncStepChain { value, step_closure }` /
        // `AsyncStepDone { value, step_closure }` / `AsyncFirstCall {
        // step_closure }`, whose inner `value`/`step_closure` boxes are
        // the *only* place the resumed-step body's `LocalGet(N)` of a
        // module-level `const` arrow appears in the post-transform HIR.
        // Skipping the descent meant the module-globals pre-walk in
        // `compile_module` (line ~1568) never saw the reference, never
        // emitted `@perry_global_<mod>__N`, and the LocalGet lowering in
        // `expr.rs` (line ~1268) fell through every check (no local /
        // capture / module_global / box) to the soft-fallback
        // `double_literal(0.0)`. Codegen then emitted `js_closure_call1(
        // 0, arg)` and the runtime SIGSEGVed dereferencing the null
        // closure pointer (the short-string "Error" address shape in
        // #859's lldb capture is downstream — the runtime walks the
        // pending-promise reject chain trying to surface a TypeError
        // and the next short-string operand lands in x19). The #894
        // explicit arms above continue to handle their cases first;
        // every variant they don't list falls through to the walker
        // here, so RegisterClass family doesn't double-walk.
        _ => {
            perry_hir::walker::walk_expr_children(e, &mut |sub| {
                collect_ref_ids_in_expr(sub, out);
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Integer-valued local detection
// ---------------------------------------------------------------------------

/// Collect LocalIds that are provably integer-valued for the lifetime of the
/// function. Used by `BinaryOp::Mod` lowering to emit integer modulo
/// (`fptosi → srem → sitofp`) instead of `frem double`, which lowers to a
/// libm `fmod()` call on ARM (no hardware instruction) and costs ~15ns per
/// iteration. Also used as the gate for allocating parallel i32 slots that
/// issue #48 leans on to skip the `fadd → fcvtzs → scvtf` round-trip on
/// `sum = (sum + i) | 0` style accumulator writes.
///
/// A local qualifies iff:
///   1. It's declared with `Let { init: Some(Expr::Integer(_)) }` — i.e. it
///      starts as a whole number, not a fraction.
///   2. Every `Expr::LocalSet(id, rhs)` has an int32-producing rhs — see
///      `is_int32_producing_expr`. `Expr::Update { id, .. }` (++/--) is
///      always permitted since it trivially preserves integer-ness.
///
/// Closure captures: writes from inside a closure body go through `LocalSet`
/// with a rhs that's typically not int32-producing, so mutably-captured
/// locals naturally fall out. Read-only captures remain qualified.
pub fn is_clamp_call(e: &perry_hir::Expr, clamp_fn_ids: &HashSet<u32>) -> bool {
    if let perry_hir::Expr::Call { callee, .. } = e {
        if let perry_hir::Expr::FuncRef(fid) = callee.as_ref() {
            return clamp_fn_ids.contains(fid);
        }
    }
    false
}
