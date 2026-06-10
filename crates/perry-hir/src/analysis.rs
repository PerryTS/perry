//! Analysis functions for HIR expressions and statements.
//!
//! Contains functions for collecting local references, tracking assigned locals,
//! checking `this` usage, and identifying builtin functions.

use perry_types::LocalId;

use crate::ir::*;
use crate::walker::{walk_expr_children, walk_expr_children_mut};

mod builtins;
pub(crate) use builtins::{
    builtin_constructor_length, builtin_global_function_length, builtin_static_function_length,
    is_builtin_function, is_builtin_global_value_name, is_builtin_static_function_member,
};

mod uses_this;
pub(crate) use uses_this::{
    closure_uses_new_target, closure_uses_this, uses_new_target_expr, uses_new_target_stmt,
    uses_this_expr, uses_this_stmt,
};

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
        Stmt::PreallocateBoxes(_) => {
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
        Stmt::PreallocateBoxes(_) => {
            // Slot+box allocation; no assignment to an outer variable.
        }
    }
}

/// Collect all local IDs that are assigned to in an expression
pub(crate) fn collect_assigned_locals_expr(expr: &Expr, assigned: &mut Vec<LocalId>) {
    match expr {
        Expr::LocalSet(id, value) => {
            // This is an assignment to a local variable
            assigned.push(*id);
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            collect_assigned_locals_expr(left, assigned);
            collect_assigned_locals_expr(right, assigned);
        }
        Expr::Unary { operand, .. } => {
            collect_assigned_locals_expr(operand, assigned);
        }
        Expr::Call { callee, args, .. } => {
            collect_assigned_locals_expr(callee, assigned);
            for arg in args {
                collect_assigned_locals_expr(arg, assigned);
            }
        }
        Expr::PropertyGet { object, .. } => {
            collect_assigned_locals_expr(object, assigned);
        }
        Expr::PropertySet { object, value, .. } => {
            collect_assigned_locals_expr(object, assigned);
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::PropertyUpdate { object, .. } => {
            collect_assigned_locals_expr(object, assigned);
        }
        Expr::IndexGet { object, index } => {
            collect_assigned_locals_expr(object, assigned);
            collect_assigned_locals_expr(index, assigned);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            collect_assigned_locals_expr(object, assigned);
            collect_assigned_locals_expr(index, assigned);
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::IndexUpdate { object, index, .. } => {
            collect_assigned_locals_expr(object, assigned);
            collect_assigned_locals_expr(index, assigned);
        }
        Expr::Array(elements) => {
            for elem in elements {
                collect_assigned_locals_expr(elem, assigned);
            }
        }
        Expr::ArraySpread(elements) => {
            for elem in elements {
                match elem {
                    ArrayElement::Expr(e) => collect_assigned_locals_expr(e, assigned),
                    ArrayElement::Spread(e) => collect_assigned_locals_expr(e, assigned),
                    ArrayElement::Hole => {}
                }
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_assigned_locals_expr(condition, assigned);
            collect_assigned_locals_expr(then_expr, assigned);
            collect_assigned_locals_expr(else_expr, assigned);
        }
        Expr::New { args, .. } => {
            for arg in args {
                collect_assigned_locals_expr(arg, assigned);
            }
        }
        Expr::Closure { .. } => {
            // Don't recurse into nested closures - assignments there are local to that closure
        }
        Expr::Await(inner) => {
            collect_assigned_locals_expr(inner, assigned);
        }
        Expr::Sequence(exprs) => {
            for e in exprs {
                collect_assigned_locals_expr(e, assigned);
            }
        }
        Expr::SuperCall(args) => {
            for arg in args {
                collect_assigned_locals_expr(arg, assigned);
            }
        }
        Expr::SuperMethodCall { args, .. } => {
            for arg in args {
                collect_assigned_locals_expr(arg, assigned);
            }
        }
        Expr::ObjectSuperPropertyGet {
            home,
            key,
            receiver,
        } => {
            collect_assigned_locals_expr(home, assigned);
            collect_assigned_locals_expr(key, assigned);
            collect_assigned_locals_expr(receiver, assigned);
        }
        Expr::SuperPropertySet { key, value, .. } => {
            collect_assigned_locals_expr(key, assigned);
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::ObjectSuperPropertySet {
            home,
            key,
            value,
            receiver,
        } => {
            collect_assigned_locals_expr(home, assigned);
            collect_assigned_locals_expr(key, assigned);
            collect_assigned_locals_expr(value, assigned);
            collect_assigned_locals_expr(receiver, assigned);
        }
        Expr::ObjectSuperMethodCall {
            home,
            key,
            receiver,
            args,
        } => {
            collect_assigned_locals_expr(home, assigned);
            collect_assigned_locals_expr(key, assigned);
            collect_assigned_locals_expr(receiver, assigned);
            for arg in args {
                collect_assigned_locals_expr(arg, assigned);
            }
        }
        Expr::Update { id, .. } => {
            // Update is an assignment
            assigned.push(*id);
        }
        // File system operations
        Expr::FsReadFileSync(path) => {
            collect_assigned_locals_expr(path, assigned);
        }
        Expr::FsWriteFileSync(path, content) => {
            collect_assigned_locals_expr(path, assigned);
            collect_assigned_locals_expr(content, assigned);
        }
        Expr::FsExistsSync(path)
        | Expr::FsMkdirSync(path)
        | Expr::FsUnlinkSync(path)
        | Expr::FsReadFileBinary(path)
        | Expr::FsRmRecursive(path) => {
            collect_assigned_locals_expr(path, assigned);
        }
        Expr::FsAppendFileSync(path, content) => {
            collect_assigned_locals_expr(path, assigned);
            collect_assigned_locals_expr(content, assigned);
        }
        Expr::ChildProcessSpawnBackground {
            command,
            args,
            log_file,
            env_json,
        } => {
            collect_assigned_locals_expr(command, assigned);
            if let Some(a) = args {
                collect_assigned_locals_expr(a, assigned);
            }
            collect_assigned_locals_expr(log_file, assigned);
            if let Some(e) = env_json {
                collect_assigned_locals_expr(e, assigned);
            }
        }
        Expr::ChildProcessGetProcessStatus(h) | Expr::ChildProcessKillProcess(h) => {
            collect_assigned_locals_expr(h, assigned);
        }
        // Path operations
        Expr::PathJoin(a, b)
        | Expr::PathMatchesGlob(a, b)
        | Expr::PathResolveJoin(a, b)
        | Expr::PathWin32Join(a, b) => {
            collect_assigned_locals_expr(a, assigned);
            collect_assigned_locals_expr(b, assigned);
        }
        Expr::PathDirname(path)
        | Expr::PathBasename(path)
        | Expr::PathExtname(path)
        | Expr::PathResolve(path)
        | Expr::PathIsAbsolute(path)
        | Expr::PathToNamespacedPath(path)
        | Expr::FileURLToPath(path) => {
            collect_assigned_locals_expr(path, assigned);
        }
        Expr::PathWin32 { args, .. } => {
            for e in args {
                collect_assigned_locals_expr(e, assigned);
            }
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
        }
        Expr::ArrayPop(_array_id) | Expr::ArrayShift(_array_id) => {
            // These modify the array but don't reallocate
        }
        Expr::ArrayIndexOf { array, value, .. } | Expr::ArrayIncludes { array, value, .. } => {
            collect_assigned_locals_expr(array, assigned);
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::ArraySlice { array, start, end } => {
            collect_assigned_locals_expr(array, assigned);
            collect_assigned_locals_expr(start, assigned);
            if let Some(e) = end {
                collect_assigned_locals_expr(e, assigned);
            }
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
        }
        Expr::ArrayForEach { array, callback }
        | Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback } => {
            collect_assigned_locals_expr(array, assigned);
            collect_assigned_locals_expr(callback, assigned);
        }
        Expr::ArraySort { array, comparator } => {
            collect_assigned_locals_expr(array, assigned);
            collect_assigned_locals_expr(comparator, assigned);
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
            collect_assigned_locals_expr(array, assigned);
            collect_assigned_locals_expr(callback, assigned);
            if let Some(init) = initial {
                collect_assigned_locals_expr(init, assigned);
            }
        }
        Expr::ArrayToReversed { array } => {
            collect_assigned_locals_expr(array, assigned);
        }
        Expr::ArrayToSorted { array, comparator } => {
            collect_assigned_locals_expr(array, assigned);
            if let Some(cmp) = comparator {
                collect_assigned_locals_expr(cmp, assigned);
            }
        }
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => {
            collect_assigned_locals_expr(array, assigned);
            collect_assigned_locals_expr(start, assigned);
            collect_assigned_locals_expr(delete_count, assigned);
            for item in items {
                collect_assigned_locals_expr(item, assigned);
            }
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            collect_assigned_locals_expr(array, assigned);
            collect_assigned_locals_expr(index, assigned);
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::ArrayReverseValue { receiver } => {
            if let Expr::LocalGet(id) = receiver.as_ref() {
                assigned.push(*id);
            }
            collect_assigned_locals_expr(receiver, assigned);
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
        }
        Expr::ArrayCopyWithinValue {
            receiver,
            target,
            start,
            end,
        } => {
            collect_assigned_locals_expr(receiver, assigned);
            collect_assigned_locals_expr(target, assigned);
            collect_assigned_locals_expr(start, assigned);
            if let Some(e) = end {
                collect_assigned_locals_expr(e, assigned);
            }
        }
        Expr::ArrayEntries(array) | Expr::ArrayKeys(array) | Expr::ArrayValues(array) => {
            collect_assigned_locals_expr(array, assigned);
        }
        Expr::ArrayJoin { array, separator } => {
            collect_assigned_locals_expr(array, assigned);
            if let Some(sep) = separator {
                collect_assigned_locals_expr(sep, assigned);
            }
        }
        Expr::ArrayFlat { array } => {
            collect_assigned_locals_expr(array, assigned);
        }
        // Native module calls
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                collect_assigned_locals_expr(obj, assigned);
            }
            for arg in args {
                collect_assigned_locals_expr(arg, assigned);
            }
        }
        // Static member access
        Expr::StaticFieldGet { .. } => {}
        Expr::StaticFieldSet { value, .. } => {
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::StaticMethodCall { args, .. } => {
            for arg in args {
                collect_assigned_locals_expr(arg, assigned);
            }
        }
        // String methods
        Expr::StringSplit(string, delimiter) => {
            collect_assigned_locals_expr(string, assigned);
            collect_assigned_locals_expr(delimiter, assigned);
        }
        Expr::StringFromCharCode(code) | Expr::StringFromCharCodeSpread(code) => {
            collect_assigned_locals_expr(code, assigned);
        }
        // Map operations
        Expr::MapNew => {}
        Expr::MapNewFromArray(expr) => {
            collect_assigned_locals_expr(expr, assigned);
        }
        Expr::MapSet { map, key, value } => {
            collect_assigned_locals_expr(map, assigned);
            collect_assigned_locals_expr(key, assigned);
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::MapGet { map, key } | Expr::MapHas { map, key } | Expr::MapDelete { map, key } => {
            collect_assigned_locals_expr(map, assigned);
            collect_assigned_locals_expr(key, assigned);
        }
        Expr::MapSize(map)
        | Expr::MapClear(map)
        | Expr::MapEntries(map)
        | Expr::MapKeys(map)
        | Expr::MapValues(map) => {
            collect_assigned_locals_expr(map, assigned);
        }
        // Set operations
        Expr::SetNew => {}
        Expr::SetNewFromArray(expr) => {
            collect_assigned_locals_expr(expr, assigned);
        }
        Expr::SetAdd { set_id, value } => {
            assigned.push(*set_id); // Set is modified by add
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
            collect_assigned_locals_expr(set, assigned);
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::SetSize(set) | Expr::SetClear(set) | Expr::SetValues(set) => {
            collect_assigned_locals_expr(set, assigned);
        }
        // JSON operations
        Expr::JsonParse(expr)
        | Expr::JsonStringify(expr)
        | Expr::JsonRawJson(expr)
        | Expr::JsonIsRawJson(expr) => {
            collect_assigned_locals_expr(expr, assigned);
        }
        // Math operations
        Expr::MathFloor(expr)
        | Expr::MathCeil(expr)
        | Expr::MathRound(expr)
        | Expr::MathAbs(expr)
        | Expr::MathSqrt(expr)
        | Expr::MathLog(expr)
        | Expr::MathLog2(expr)
        | Expr::MathLog10(expr) => {
            collect_assigned_locals_expr(expr, assigned);
        }
        Expr::MathPow(base, exp) | Expr::MathImul(base, exp) => {
            collect_assigned_locals_expr(base, assigned);
            collect_assigned_locals_expr(exp, assigned);
        }
        Expr::MathMin(args) | Expr::MathMax(args) => {
            for arg in args {
                collect_assigned_locals_expr(arg, assigned);
            }
        }
        Expr::MathMinSpread(expr) | Expr::MathMaxSpread(expr) => {
            collect_assigned_locals_expr(expr, assigned);
        }
        Expr::MathRandom => {}
        // Crypto operations
        Expr::CryptoRandomBytes(expr) | Expr::CryptoSha256(expr) | Expr::CryptoMd5(expr) => {
            collect_assigned_locals_expr(expr, assigned);
        }
        Expr::CryptoRandomUUID => {}
        Expr::CryptoRandomUUIDv7 => {}
        // OS operations (no assignments)
        Expr::OsPlatform
        | Expr::OsArch
        | Expr::OsHostname
        | Expr::OsHomedir
        | Expr::OsTmpdir
        | Expr::OsTotalmem
        | Expr::OsFreemem
        | Expr::OsUptime
        | Expr::OsType
        | Expr::OsRelease
        | Expr::OsCpus
        | Expr::OsNetworkInterfaces
        | Expr::OsUserInfo
        | Expr::OsUserInfoBuffer
        | Expr::OsEOL
        | Expr::OsDevNull
        | Expr::OsAvailableParallelism
        | Expr::OsEndianness
        | Expr::OsLoadavg
        | Expr::OsMachine
        | Expr::OsVersion => {}
        // Buffer operations
        Expr::BufferFrom { data, encoding } => {
            collect_assigned_locals_expr(data, assigned);
            if let Some(enc) = encoding {
                collect_assigned_locals_expr(enc, assigned);
            }
        }
        Expr::BufferFromArrayBuffer {
            data,
            byte_offset,
            length,
        } => {
            collect_assigned_locals_expr(data, assigned);
            collect_assigned_locals_expr(byte_offset, assigned);
            if let Some(len) = length {
                collect_assigned_locals_expr(len, assigned);
            }
        }
        Expr::BufferAlloc {
            size,
            fill,
            encoding,
        } => {
            collect_assigned_locals_expr(size, assigned);
            if let Some(f) = fill {
                collect_assigned_locals_expr(f, assigned);
            }
            if let Some(e) = encoding {
                collect_assigned_locals_expr(e, assigned);
            }
        }
        Expr::BufferAllocUnsafe(expr)
        | Expr::BufferConcat(expr)
        | Expr::BufferIsBuffer(expr)
        | Expr::BufferIsEncoding(expr)
        | Expr::BufferLength(expr) => {
            collect_assigned_locals_expr(expr, assigned);
        }
        Expr::BufferConcatWithLength { list, total_length } => {
            collect_assigned_locals_expr(list, assigned);
            collect_assigned_locals_expr(total_length, assigned);
        }
        Expr::BufferByteLength { data, encoding } => {
            collect_assigned_locals_expr(data, assigned);
            if let Some(enc) = encoding {
                collect_assigned_locals_expr(enc, assigned);
            }
        }
        Expr::BufferToString { buffer, encoding } => {
            collect_assigned_locals_expr(buffer, assigned);
            if let Some(enc) = encoding {
                collect_assigned_locals_expr(enc, assigned);
            }
        }
        Expr::BufferFill { buffer, value } => {
            collect_assigned_locals_expr(buffer, assigned);
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::BufferSlice { buffer, start, end } => {
            collect_assigned_locals_expr(buffer, assigned);
            if let Some(s) = start {
                collect_assigned_locals_expr(s, assigned);
            }
            if let Some(e) = end {
                collect_assigned_locals_expr(e, assigned);
            }
        }
        Expr::BufferCopy {
            source,
            target,
            target_start,
            source_start,
            source_end,
        } => {
            collect_assigned_locals_expr(source, assigned);
            collect_assigned_locals_expr(target, assigned);
            if let Some(ts) = target_start {
                collect_assigned_locals_expr(ts, assigned);
            }
            if let Some(ss) = source_start {
                collect_assigned_locals_expr(ss, assigned);
            }
            if let Some(se) = source_end {
                collect_assigned_locals_expr(se, assigned);
            }
        }
        Expr::BufferWrite {
            buffer,
            string,
            offset,
            encoding,
        } => {
            collect_assigned_locals_expr(buffer, assigned);
            collect_assigned_locals_expr(string, assigned);
            if let Some(o) = offset {
                collect_assigned_locals_expr(o, assigned);
            }
            if let Some(e) = encoding {
                collect_assigned_locals_expr(e, assigned);
            }
        }
        Expr::BufferEquals { buffer, other } => {
            collect_assigned_locals_expr(buffer, assigned);
            collect_assigned_locals_expr(other, assigned);
        }
        Expr::BufferIndexGet { buffer, index } => {
            collect_assigned_locals_expr(buffer, assigned);
            collect_assigned_locals_expr(index, assigned);
        }
        Expr::BufferIndexSet {
            buffer,
            index,
            value,
        } => {
            collect_assigned_locals_expr(buffer, assigned);
            collect_assigned_locals_expr(index, assigned);
            collect_assigned_locals_expr(value, assigned);
        }
        // Child Process operations
        Expr::ChildProcessExecSync { command, options } => {
            collect_assigned_locals_expr(command, assigned);
            if let Some(opts) = options {
                collect_assigned_locals_expr(opts, assigned);
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
            collect_assigned_locals_expr(command, assigned);
            if let Some(a) = args {
                collect_assigned_locals_expr(a, assigned);
            }
            if let Some(opts) = options {
                collect_assigned_locals_expr(opts, assigned);
            }
        }
        Expr::ChildProcessFork {
            module,
            args,
            options,
        } => {
            collect_assigned_locals_expr(module, assigned);
            if let Some(a) = args {
                collect_assigned_locals_expr(a, assigned);
            }
            if let Some(opts) = options {
                collect_assigned_locals_expr(opts, assigned);
            }
        }
        Expr::ChildProcessExec {
            command,
            options,
            callback,
        } => {
            collect_assigned_locals_expr(command, assigned);
            if let Some(opts) = options {
                collect_assigned_locals_expr(opts, assigned);
            }
            if let Some(cb) = callback {
                collect_assigned_locals_expr(cb, assigned);
            }
        }
        // Net operations
        Expr::NetCreateServer {
            options,
            connection_listener,
        } => {
            if let Some(opts) = options {
                collect_assigned_locals_expr(opts, assigned);
            }
            if let Some(cl) = connection_listener {
                collect_assigned_locals_expr(cl, assigned);
            }
        }
        Expr::NetCreateConnection {
            port,
            host,
            connect_listener,
        }
        | Expr::NetConnect {
            port,
            host,
            connect_listener,
        } => {
            collect_assigned_locals_expr(port, assigned);
            if let Some(h) = host {
                collect_assigned_locals_expr(h, assigned);
            }
            if let Some(cl) = connect_listener {
                collect_assigned_locals_expr(cl, assigned);
            }
        }
        // Date operations
        Expr::DateNow => {}
        Expr::DateNew(args) => {
            for a in args {
                collect_assigned_locals_expr(a, assigned);
            }
        }
        Expr::DateGetTime(date)
        | Expr::DateToISOString(date)
        | Expr::DateGetFullYear(date)
        | Expr::DateGetMonth(date)
        | Expr::DateGetDate(date)
        | Expr::DateGetDay(date)
        | Expr::DateGetHours(date)
        | Expr::DateGetMinutes(date)
        | Expr::DateGetSeconds(date)
        | Expr::DateGetMilliseconds(date) => {
            collect_assigned_locals_expr(date, assigned);
        }
        // URL operations
        Expr::UrlNew { url, base } => {
            collect_assigned_locals_expr(url, assigned);
            if let Some(base_expr) = base {
                collect_assigned_locals_expr(base_expr, assigned);
            }
        }
        Expr::UrlPatternNew { input, base } => {
            collect_assigned_locals_expr(input, assigned);
            if let Some(base_expr) = base {
                collect_assigned_locals_expr(base_expr, assigned);
            }
        }
        Expr::UrlGetHref(url)
        | Expr::UrlGetPathname(url)
        | Expr::UrlGetProtocol(url)
        | Expr::UrlGetHost(url)
        | Expr::UrlGetHostname(url)
        | Expr::UrlGetPort(url)
        | Expr::UrlGetSearch(url)
        | Expr::UrlGetHash(url)
        | Expr::UrlGetOrigin(url)
        | Expr::UrlGetSearchParams(url)
        | Expr::UrlParse(url)
        | Expr::UrlInstanceToString(url)
        | Expr::UrlInstanceToJSON(url) => {
            collect_assigned_locals_expr(url, assigned);
        }
        Expr::UrlCanParse(url) => {
            collect_assigned_locals_expr(url, assigned);
        }
        Expr::UrlCanParseWithBase { input, base } => {
            collect_assigned_locals_expr(input, assigned);
            collect_assigned_locals_expr(base, assigned);
        }
        Expr::UrlParseWithBase { input, base } => {
            collect_assigned_locals_expr(input, assigned);
            collect_assigned_locals_expr(base, assigned);
        }
        // URLSearchParams operations
        Expr::UrlSearchParamsNew(init) => {
            if let Some(init_expr) = init {
                collect_assigned_locals_expr(init_expr, assigned);
            }
        }
        Expr::UrlSearchParamsMissingArgs { params, args, .. } => {
            collect_assigned_locals_expr(params, assigned);
            for arg in args {
                collect_assigned_locals_expr(arg, assigned);
            }
        }
        Expr::UrlSearchParamsGet { params, name }
        | Expr::UrlSearchParamsGetAll { params, name } => {
            collect_assigned_locals_expr(params, assigned);
            collect_assigned_locals_expr(name, assigned);
        }
        Expr::UrlSearchParamsHas {
            params,
            name,
            value,
        }
        | Expr::UrlSearchParamsDelete {
            params,
            name,
            value,
        } => {
            collect_assigned_locals_expr(params, assigned);
            collect_assigned_locals_expr(name, assigned);
            if let Some(v) = value {
                collect_assigned_locals_expr(v, assigned);
            }
        }
        Expr::UrlSearchParamsSet {
            params,
            name,
            value,
        }
        | Expr::UrlSearchParamsAppend {
            params,
            name,
            value,
        } => {
            collect_assigned_locals_expr(params, assigned);
            collect_assigned_locals_expr(name, assigned);
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::UrlSearchParamsForEach {
            params,
            callback,
            this_arg,
        } => {
            collect_assigned_locals_expr(params, assigned);
            collect_assigned_locals_expr(callback, assigned);
            if let Some(this_arg) = this_arg {
                collect_assigned_locals_expr(this_arg, assigned);
            }
        }
        Expr::UrlSearchParamsToString(params)
        | Expr::UrlSearchParamsEntries(params)
        | Expr::UrlSearchParamsKeys(params)
        | Expr::UrlSearchParamsValues(params)
        | Expr::UrlSearchParamsSort(params) => {
            collect_assigned_locals_expr(params, assigned);
        }
        Expr::GlobalSet(_, value) => {
            collect_assigned_locals_expr(value, assigned);
        }
        // Terminal expressions that don't have children or don't assign
        Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::FuncRef(_)
        | Expr::ExternFuncRef { .. }
        | Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. }
        | Expr::NewTarget
        | Expr::ClassRef(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::BigInt(_)
        | Expr::Object(_)
        | Expr::TypeOf(_)
        | Expr::InstanceOf { .. }
        | Expr::EnumMember { .. }
        | Expr::This
        | Expr::Null
        | Expr::Undefined
        | Expr::EnvGet(_)
        | Expr::ProcessUptime
        | Expr::ProcessCwd
        | Expr::ProcessMemoryUsage
        | Expr::ProcessEnv
        | Expr::GlobalThisExpr
        | Expr::NativeModuleRef(_)
        | Expr::RegExp { .. } => {}
        Expr::ObjectKeys(obj)
        | Expr::ForInKeys(obj)
        | Expr::ObjectValues(obj)
        | Expr::ObjectEntries(obj) => {
            collect_assigned_locals_expr(obj, assigned);
        }
        Expr::ObjectGroupBy { items, key_fn } | Expr::MapGroupBy { items, key_fn } => {
            collect_assigned_locals_expr(items, assigned);
            collect_assigned_locals_expr(key_fn, assigned);
        }
        Expr::ArrayIsArray(value)
        | Expr::ArrayFrom(value)
        | Expr::ArrayFromArrayLikeHoley(value) => {
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::ArrayFromMapped {
            iterable,
            map_fn,
            this_arg,
        } => {
            collect_assigned_locals_expr(iterable, assigned);
            collect_assigned_locals_expr(map_fn, assigned);
            if let Some(t) = this_arg {
                collect_assigned_locals_expr(t, assigned);
            }
        }
        Expr::RegExpTest { regex, string } => {
            collect_assigned_locals_expr(regex, assigned);
            collect_assigned_locals_expr(string, assigned);
        }
        Expr::RegExpDynamic { pattern, flags } => {
            collect_assigned_locals_expr(pattern, assigned);
            if let Some(f) = flags {
                collect_assigned_locals_expr(f, assigned);
            }
        }
        Expr::StringMatch { string, regex } => {
            collect_assigned_locals_expr(string, assigned);
            collect_assigned_locals_expr(regex, assigned);
        }
        Expr::StringReplace {
            string,
            pattern,
            replacement,
        } => {
            collect_assigned_locals_expr(string, assigned);
            collect_assigned_locals_expr(pattern, assigned);
            collect_assigned_locals_expr(replacement, assigned);
        }
        Expr::ParseInt { string, radix } => {
            collect_assigned_locals_expr(string, assigned);
            if let Some(r) = radix {
                collect_assigned_locals_expr(r, assigned);
            }
        }
        Expr::ParseFloat(string) => {
            collect_assigned_locals_expr(string, assigned);
        }
        Expr::NumberCoerce(value) => {
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::BigIntCoerce(value) => {
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::StringCoerce(value) => {
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::ObjectCoerce(value) => {
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::BooleanCoerce(value) => {
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::IsNaN(value) => {
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::IsUndefinedOrBareNan(value) => {
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::IsFinite(value) => {
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::StaticPluginResolve(value) => {
            collect_assigned_locals_expr(value, assigned);
        }
        // JS runtime expressions
        Expr::JsLoadModule { .. } => {}
        Expr::JsGetExport { module_handle, .. } => {
            collect_assigned_locals_expr(module_handle, assigned);
        }
        Expr::JsCallFunction {
            module_handle,
            args,
            ..
        } => {
            collect_assigned_locals_expr(module_handle, assigned);
            for arg in args {
                collect_assigned_locals_expr(arg, assigned);
            }
        }
        Expr::JsCallMethod { object, args, .. } => {
            collect_assigned_locals_expr(object, assigned);
            for arg in args {
                collect_assigned_locals_expr(arg, assigned);
            }
        }
        // #853: an earlier arm in this match (lines 682-695) already
        // covers every Expr::Os* variant. The duplicate arm here was
        // dead — removed.
        // Delete operator
        Expr::Delete(inner) => {
            collect_assigned_locals_expr(inner, assigned);
        }
        // Error operations
        Expr::ErrorNew(msg) => {
            if let Some(m) = msg {
                collect_assigned_locals_expr(m, assigned);
            }
        }
        Expr::ErrorMessage(err) => {
            collect_assigned_locals_expr(err, assigned);
        }
        Expr::ErrorNewWithCause { message, cause: b }
        | Expr::ErrorNewWithOptions {
            message,
            options: b,
            ..
        } => {
            collect_assigned_locals_expr(message, assigned);
            collect_assigned_locals_expr(b, assigned);
        }
        Expr::TypeErrorNew(m)
        | Expr::RangeErrorNew(m)
        | Expr::ReferenceErrorNew(m)
        | Expr::SyntaxErrorNew(m) => {
            collect_assigned_locals_expr(m, assigned);
        }
        Expr::AggregateErrorNew {
            errors,
            message,
            options,
        } => {
            collect_assigned_locals_expr(errors, assigned);
            collect_assigned_locals_expr(message, assigned);
            options
                .iter()
                .for_each(|o| collect_assigned_locals_expr(o, assigned));
        }
        // Uint8Array operations
        Expr::Uint8ArrayNew(size) => {
            if let Some(s) = size {
                collect_assigned_locals_expr(s, assigned);
            }
        }
        Expr::Uint8ArrayFrom(data) | Expr::Uint8ArrayLength(data) => {
            collect_assigned_locals_expr(data, assigned);
        }
        Expr::Uint8ArrayGet { array, index } => {
            collect_assigned_locals_expr(array, assigned);
            collect_assigned_locals_expr(index, assigned);
        }
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => {
            collect_assigned_locals_expr(array, assigned);
            collect_assigned_locals_expr(index, assigned);
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::TypedArrayNew { arg, .. } => {
            if let Some(a) = arg {
                collect_assigned_locals_expr(a, assigned);
            }
        }
        Expr::NativeArenaAlloc(byte_length) | Expr::NativeArenaDispose(byte_length) => {
            collect_assigned_locals_expr(byte_length, assigned);
        }
        Expr::NativeArenaView {
            owner,
            byte_offset,
            length,
            ..
        } => {
            collect_assigned_locals_expr(owner, assigned);
            collect_assigned_locals_expr(byte_offset, assigned);
            collect_assigned_locals_expr(length, assigned);
        }
        Expr::NativePodView {
            owner,
            byte_offset,
            count,
            ..
        } => {
            collect_assigned_locals_expr(owner, assigned);
            collect_assigned_locals_expr(byte_offset, assigned);
            collect_assigned_locals_expr(count, assigned);
        }
        Expr::NativeMemoryFillU32 { view, value } => {
            collect_assigned_locals_expr(view, assigned);
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::NativeMemoryCopy { dst, src } => {
            collect_assigned_locals_expr(dst, assigned);
            collect_assigned_locals_expr(src, assigned);
        }
        // Dynamic env access
        Expr::EnvGetDynamic(key) => {
            collect_assigned_locals_expr(key, assigned);
        }
        // JS runtime expressions with sub-expressions
        Expr::JsGetProperty { object, .. } => {
            collect_assigned_locals_expr(object, assigned);
        }
        Expr::JsSetProperty { object, value, .. } => {
            collect_assigned_locals_expr(object, assigned);
            collect_assigned_locals_expr(value, assigned);
        }
        Expr::JsNew {
            module_handle,
            args,
            ..
        } => {
            collect_assigned_locals_expr(module_handle, assigned);
            for arg in args {
                collect_assigned_locals_expr(arg, assigned);
            }
        }
        Expr::JsNewFromHandle { constructor, args } => {
            collect_assigned_locals_expr(constructor, assigned);
            for arg in args {
                collect_assigned_locals_expr(arg, assigned);
            }
        }
        Expr::JsCreateCallback { closure, .. } => {
            collect_assigned_locals_expr(closure, assigned);
        }
        // Spread call expressions
        Expr::CallSpread { callee, args, .. } => {
            collect_assigned_locals_expr(callee, assigned);
            for arg in args {
                match arg {
                    CallArg::Expr(e) | CallArg::Spread(e) => {
                        collect_assigned_locals_expr(e, assigned)
                    }
                }
            }
        }
        // Void operator
        Expr::Void(inner) => {
            collect_assigned_locals_expr(inner, assigned);
        }
        // Yield expression
        Expr::Yield { value, .. } => {
            if let Some(v) = value {
                collect_assigned_locals_expr(v, assigned);
            }
        }
        // Dynamic new expression
        Expr::NewDynamic { callee, args } => {
            collect_assigned_locals_expr(callee, assigned);
            for arg in args {
                collect_assigned_locals_expr(arg, assigned);
            }
        }
        // Object rest destructuring
        Expr::ObjectRest { object, .. } => {
            collect_assigned_locals_expr(object, assigned);
        }
        // Fetch with options
        Expr::FetchWithOptions {
            url,
            method,
            body,
            headers,
            headers_dynamic,
        } => {
            collect_assigned_locals_expr(url, assigned);
            collect_assigned_locals_expr(method, assigned);
            collect_assigned_locals_expr(body, assigned);
            for (_, v) in headers {
                collect_assigned_locals_expr(v, assigned);
            }
            if let Some(hd) = headers_dynamic {
                collect_assigned_locals_expr(hd, assigned);
            }
        }
        Expr::FetchGetWithAuth { url, auth_header } => {
            collect_assigned_locals_expr(url, assigned);
            collect_assigned_locals_expr(auth_header, assigned);
        }
        Expr::FetchPostWithAuth {
            url,
            auth_header,
            body,
        } => {
            collect_assigned_locals_expr(url, assigned);
            collect_assigned_locals_expr(auth_header, assigned);
            collect_assigned_locals_expr(body, assigned);
        }
        // Catch-all for any other terminal expressions
        _ => {}
    }
}

/// Rewrite all `Expr::This` references inside a block of statements to
/// `Expr::LocalGet(this_id)`. Used to lift class generator methods
/// (`*[Symbol.iterator]()`) to a top-level function with `this` as an
/// explicit parameter.
///
/// Does NOT recurse into nested closures — those have their own `this`
/// binding and should keep referencing the outer class context.
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
        Expr::NewDynamic { callee, args } => {
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
        Expr::InstanceOf { expr: inner, .. } => replace_this_in_expr(inner, this_id),
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
