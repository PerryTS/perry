//! ID scanning helpers: find max LocalId / FuncId across module.

use super::*;

/// Find the maximum local ID used in the module.
pub fn compute_max_local_id(module: &Module) -> LocalId {
    let mut max_id: LocalId = 0;
    for func in &module.functions {
        for param in &func.params {
            max_id = max_id.max(param.id);
        }
        scan_stmts_for_max_local(&func.body, &mut max_id);
    }
    for stmt in &module.init {
        scan_stmt_for_max_local(stmt, &mut max_id);
    }
    for global in &module.globals {
        max_id = max_id.max(global.id);
        // #5293: a module-global initializer (`const f = (x) => ...`) holds a
        // closure whose params/body live in this LocalId namespace. The
        // de-duplicated copies that routed through here did not all scan it,
        // so fold it in — a too-high max is always safe, a missed id collides.
        if let Some(init) = &global.init {
            scan_expr_for_max_local(init, &mut max_id);
        }
    }
    // Also scan class member bodies — they share the LocalId namespace.
    // The v0.5.323 issue #212 fix allocates method-local rebind ids per
    // class method per captured outer local; without this scan, the
    // generator transform's freshly-allocated state/done/sent/wrapper
    // ids could collide with those rebind ids and corrupt unrelated
    // class-method codegen.
    for class in &module.classes {
        for method in &class.methods {
            for param in &method.params {
                max_id = max_id.max(param.id);
            }
            scan_stmts_for_max_local(&method.body, &mut max_id);
        }
        for static_method in &class.static_methods {
            for param in &static_method.params {
                max_id = max_id.max(param.id);
            }
            scan_stmts_for_max_local(&static_method.body, &mut max_id);
        }
        for member in &class.computed_members {
            scan_expr_for_max_local(&member.key_expr, &mut max_id);
            for param in &member.function.params {
                max_id = max_id.max(param.id);
            }
            scan_stmts_for_max_local(&member.function.body, &mut max_id);
        }
        if let Some(ctor) = &class.constructor {
            for param in &ctor.params {
                max_id = max_id.max(param.id);
            }
            scan_stmts_for_max_local(&ctor.body, &mut max_id);
        }
        for getter in &class.getters {
            for param in &getter.1.params {
                max_id = max_id.max(param.id);
            }
            scan_stmts_for_max_local(&getter.1.body, &mut max_id);
        }
        for setter in &class.setters {
            for param in &setter.1.params {
                max_id = max_id.max(param.id);
            }
            scan_stmts_for_max_local(&setter.1.body, &mut max_id);
        }
        // Issue #5143 (LocalId parallel): class FIELD initializers and
        // computed-key exprs hold closures whose params/body LocalIds live in
        // this namespace but are NOT reachable through any method/ctor body.
        // `compute_max_func_id` already scans these (the #5143 FuncId fix); the
        // LocalId scan was left incomplete, so the generator/async transform
        // could synthesize state/done/sent/wrapper LocalIds that COLLIDE with a
        // field-init closure's locals and corrupt unrelated codegen (e.g. a
        // module-global/capture read resolving to the wrong value — Next.js
        // app-page-turbo `()=>X` export getters at scale). A too-high max is
        // always safe; a missed id collides.
        for field in class.fields.iter().chain(class.static_fields.iter()) {
            if let Some(init) = &field.init {
                scan_expr_for_max_local(init, &mut max_id);
            }
            if let Some(key_expr) = &field.key_expr {
                scan_expr_for_max_local(key_expr, &mut max_id);
            }
        }
        if let Some(extends_expr) = &class.extends_expr {
            scan_expr_for_max_local(extends_expr, &mut max_id);
        }
    }
    max_id
}

pub fn scan_stmts_for_max_local(stmts: &[Stmt], max_id: &mut LocalId) {
    for stmt in stmts {
        scan_stmt_for_max_local(stmt, max_id);
    }
}

pub fn scan_stmt_for_max_local(stmt: &Stmt, max_id: &mut LocalId) {
    match stmt {
        Stmt::Let { id, init, .. } => {
            *max_id = (*max_id).max(*id);
            if let Some(e) = init {
                scan_expr_for_max_local(e, max_id);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => scan_expr_for_max_local(e, max_id),
        Stmt::Return(e) => {
            if let Some(e) = e {
                scan_expr_for_max_local(e, max_id);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            scan_expr_for_max_local(condition, max_id);
            scan_stmts_for_max_local(then_branch, max_id);
            if let Some(eb) = else_branch {
                scan_stmts_for_max_local(eb, max_id);
            }
        }
        Stmt::While { condition, body } => {
            scan_expr_for_max_local(condition, max_id);
            scan_stmts_for_max_local(body, max_id);
        }
        Stmt::DoWhile { body, condition } => {
            scan_stmts_for_max_local(body, max_id);
            scan_expr_for_max_local(condition, max_id);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                scan_stmt_for_max_local(i, max_id);
            }
            if let Some(c) = condition {
                scan_expr_for_max_local(c, max_id);
            }
            if let Some(u) = update {
                scan_expr_for_max_local(u, max_id);
            }
            scan_stmts_for_max_local(body, max_id);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            scan_stmts_for_max_local(body, max_id);
            if let Some(c) = catch {
                // The catch param's LocalId only appears in the
                // `CatchClause::param` field; if the catch body never
                // reads/writes it (`catch (e) {}`), no LocalGet/LocalSet
                // carries it, and the body-only scan misses it. The
                // generator transform then allocates `__gen_state` over
                // the catch param's id — every `LocalSet(__gen_state, N)`
                // clobbers `e` and vice versa, so the state machine never
                // advances and the function returns undefined without
                // ever entering state 0. Repro: any async function with
                // `try { await … } catch (e) {}` and an unused catch
                // binding (hono's compose dispatches every middleware
                // through this exact shape).
                if let Some((id, _)) = c.param {
                    *max_id = (*max_id).max(id);
                }
                scan_stmts_for_max_local(&c.body, max_id);
            }
            if let Some(f) = finally {
                scan_stmts_for_max_local(f, max_id);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            scan_expr_for_max_local(discriminant, max_id);
            for case in cases {
                // #5293: `case (() => x)():` / `case foo[i]:` test exprs carry
                // their own LocalIds (and nested closures). The merged copies
                // scanned these; the canonical must too.
                if let Some(test) = &case.test {
                    scan_expr_for_max_local(test, max_id);
                }
                scan_stmts_for_max_local(&case.body, max_id);
            }
        }
        Stmt::Labeled { body, .. } => scan_stmt_for_max_local(body, max_id),
        // PreallocateBoxes carries raw LocalIds (the boxed locals of a
        // closure-conversion pass that ran before this scan); they are
        // usually also visible via captures lists, but max them here so the
        // scan does not depend on that invariant.
        Stmt::PreallocateBoxes(ids) => {
            for id in ids {
                *max_id = (*max_id).max(*id);
            }
        }
        _ => {}
    }
}

/// Walk an expression for any LocalIds it carries — Closure params/captures,
/// LocalGet/LocalSet, and recursively into all sub-expressions. Without this
/// scan, IIFE-style closures emitted into module init (or any
/// `Expr::Call(Closure { params: [...], body: [...] }, args)` shape) hide
/// their parameter LocalIds from `compute_max_local_id`, and the generator
/// transform's freshly-allocated `__gen_state`/`__gen_done`/`__gen_sent`
/// locals collide with them. The collision corrupts every LocalGet/LocalSet
/// in either the IIFE body or the generator state machine and produces
/// silent miscompilation or segfaults.
///
/// Per-variant handling below covers only the LocalId fields an Expr owns
/// directly; descent into sub-expressions is delegated to
/// `perry_hir::walker::walk_expr_children` (exhaustively matched, so a new
/// Expr variant that forgets the walker is a compile error). Pre-#4851 this
/// fn carried its own ad-hoc child walker with a `_ => {}` catch-all, which
/// silently skipped any variant without an explicit arm — `Expr::ObjectAssign`
/// (`Object.assign(p, { a: () => 1, b: () => 2 })`) hid its sources' closures
/// from the scan, the transform minted colliding ids, and codegen (one LLVM
/// function per id) compiled the wrong body for the async-step closure.
pub fn scan_expr_for_max_local(expr: &Expr, max_id: &mut LocalId) {
    match expr {
        Expr::LocalGet(id) | Expr::LocalSet(id, _) => *max_id = (*max_id).max(*id),
        Expr::Update { id, .. } => *max_id = (*max_id).max(*id),
        Expr::ArrayPush { array_id, .. }
        | Expr::ArrayPushSpread { array_id, .. }
        | Expr::ArrayUnshift { array_id, .. }
        | Expr::ArraySplice { array_id, .. }
        | Expr::ArrayCopyWithin { array_id, .. } => *max_id = (*max_id).max(*array_id),
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => *max_id = (*max_id).max(*id),
        Expr::SetAdd { set_id, .. } => *max_id = (*max_id).max(*set_id),
        Expr::Closure {
            params,
            body,
            captures,
            mutable_captures,
            ..
        } => {
            // Param DEFAULT exprs are visited by the walker below; the
            // body is a Stmt list the expr walker cannot see, so scan it
            // here.
            for p in params {
                *max_id = (*max_id).max(p.id);
            }
            for c in captures {
                *max_id = (*max_id).max(*c);
            }
            for c in mutable_captures {
                *max_id = (*max_id).max(*c);
            }
            scan_stmts_for_max_local(body, max_id);
        }
        _ => {}
    }
    perry_hir::walker::walk_expr_children(expr, &mut |e| scan_expr_for_max_local(e, max_id));
}

/// Find the maximum func ID used in the module.
pub fn compute_max_func_id(module: &Module) -> FuncId {
    let mut max_id: FuncId = 0;
    for func in &module.functions {
        max_id = max_id.max(func.id);
        scan_stmts_for_max_func(&func.body, &mut max_id);
    }
    for stmt in &module.init {
        scan_stmt_for_max_func(stmt, &mut max_id);
    }
    // #5293: module-global initializers (`const f = () => ...`) hold closures
    // whose FuncIds live in this namespace. `async_to_generator`'s merged copy
    // scanned global inits; fold it in so the canonical is a true superset.
    for global in &module.globals {
        if let Some(init) = &global.init {
            scan_expr_for_max_func(init, &mut max_id);
        }
    }
    // Issue #154: class member bodies share the FuncId namespace — their
    // nested closures (executors, callbacks, dispose-method bodies)
    // carry FuncIds that the iterator state-machine transform must not
    // collide with. Mirrors the v0.5.323 class-scan added to
    // `compute_max_local_id` for issue #212. Without this, an
    // `await using r = new R()` where `R[Symbol.asyncDispose]()` awaits
    // a user-built Promise has its executor closure's FuncId reused for
    // the synthesized iterator-return, and at codegen the iterator-
    // return body wins. The body reads past the (0-cap) executor's
    // capture array and `js_box_set`s a code-segment pointer → SIGBUS.
    for class in &module.classes {
        for m in class
            .methods
            .iter()
            .chain(class.static_methods.iter())
            .chain(class.computed_members.iter().map(|member| &member.function))
            .chain(class.constructor.iter())
            .chain(class.getters.iter().map(|(_, f)| f))
            .chain(class.setters.iter().map(|(_, f)| f))
        {
            scan_stmts_for_max_func(&m.body, &mut max_id);
        }
        for member in &class.computed_members {
            scan_expr_for_max_func(&member.key_expr, &mut max_id);
        }
        // Issue #5143: class FIELD initializers (`request = (input) => ...`)
        // and computed-key expressions also live in this FuncId namespace.
        // Their closures are NOT reachable through any method/constructor
        // body, so without scanning them the iterator/generator
        // state-machine transform can reuse a field-initializer closure's
        // FuncId for a synthesized step function — at codegen the step body
        // wins and the class field ends up bound to the wrong function
        // (Hono's `app.request()` returned a stray iterator closure).
        for field in class.fields.iter().chain(class.static_fields.iter()) {
            if let Some(init) = &field.init {
                scan_expr_for_max_func(init, &mut max_id);
            }
            if let Some(key_expr) = &field.key_expr {
                scan_expr_for_max_func(key_expr, &mut max_id);
            }
        }
        if let Some(extends_expr) = &class.extends_expr {
            scan_expr_for_max_func(extends_expr, &mut max_id);
        }
    }
    max_id
}

pub fn scan_stmts_for_max_func(stmts: &[Stmt], max_id: &mut FuncId) {
    for stmt in stmts {
        scan_stmt_for_max_func(stmt, max_id);
    }
}

pub fn scan_stmt_for_max_func(stmt: &Stmt, max_id: &mut FuncId) {
    match stmt {
        Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
            scan_expr_for_max_func(expr, max_id);
        }
        Stmt::Let {
            init: Some(expr), ..
        } => scan_expr_for_max_func(expr, max_id),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            scan_expr_for_max_func(condition, max_id);
            scan_stmts_for_max_func(then_branch, max_id);
            if let Some(eb) = else_branch {
                scan_stmts_for_max_func(eb, max_id);
            }
        }
        // Mirror scan_stmt_for_max_local: a closure declared inside a
        // do-while or labeled loop must still bump the max func id, or the
        // generator transform can mint a colliding func id (#1824 gap class).
        // Loop/switch HEAD expressions can nest closures too (`while
        // ((() => f())())`, `for (let g = () => 1;;)`), so scan them like
        // the local-id twin does, not just the bodies.
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            scan_expr_for_max_func(condition, max_id);
            scan_stmts_for_max_func(body, max_id);
        }
        Stmt::Labeled { body, .. } => scan_stmt_for_max_func(body, max_id),
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                scan_stmt_for_max_func(i, max_id);
            }
            if let Some(c) = condition {
                scan_expr_for_max_func(c, max_id);
            }
            if let Some(u) = update {
                scan_expr_for_max_func(u, max_id);
            }
            scan_stmts_for_max_func(body, max_id);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            scan_stmts_for_max_func(body, max_id);
            if let Some(c) = catch {
                scan_stmts_for_max_func(&c.body, max_id);
            }
            if let Some(f) = finally {
                scan_stmts_for_max_func(f, max_id);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            scan_expr_for_max_func(discriminant, max_id);
            for case in cases {
                // #5293: a closure in a switch case test (`case (() => 1)():`)
                // carries a FuncId the merged copies scanned; mirror them here.
                if let Some(test) = &case.test {
                    scan_expr_for_max_func(test, max_id);
                }
                scan_stmts_for_max_func(&case.body, max_id);
            }
        }
        _ => {}
    }
}

/// Walk an expression for any FuncIds it carries — `FuncRef`s and `Closure`s,
/// recursively through all sub-expressions and closure bodies.
///
/// Per-variant handling below covers only the FuncId fields an Expr owns
/// directly; descent into sub-expressions is delegated to
/// `perry_hir::walker::walk_expr_children` (exhaustively matched, so a new
/// Expr variant that forgets the walker is a compile error). Pre-#4851 this
/// fn carried its own ad-hoc child walker with a `_ => {}` catch-all that
/// silently skipped any variant without an explicit arm. The catch-all bit
/// repeatedly (#393, #531, #1824 each added one more arm); #4851 was the
/// same class: `Object.assign(promise, { a: () => 1, b: () => 2 })` lowers
/// to `ObjectAssign { sources: [New { __AnonShape, args: [Closure, ...] }] }`,
/// no `ObjectAssign` arm existed, so the literal's closures were invisible
/// here. `transform_generators` then minted the SAME FuncId for the
/// `__async_step` closure of a caller awaiting that function, codegen (one
/// LLVM function per FuncId) compiled the arrow body in place of the step
/// closure, and the await's continuation was silently dropped — `await
/// makeReq()` never resumed (the Stripe SDK's auto-pagination
/// `Object.assign` hit exactly this).
pub fn scan_expr_for_max_func(expr: &Expr, max_id: &mut FuncId) {
    match expr {
        Expr::FuncRef(id) => *max_id = (*max_id).max(*id),
        Expr::Closure { func_id, body, .. } => {
            // Param DEFAULT exprs are visited by the walker below; the
            // body is a Stmt list the expr walker cannot see, so scan it
            // here.
            *max_id = (*max_id).max(*func_id);
            scan_stmts_for_max_func(body, max_id);
        }
        _ => {}
    }
    perry_hir::walker::walk_expr_children(expr, &mut |e| scan_expr_for_max_func(e, max_id));
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_types::Type;

    /// #4851: `Object.assign(p, { a: () => 1, b: () => 2 })` lowers to
    /// `ObjectAssign { sources: [New { __AnonShape, args: [Closure, ...] }] }`.
    /// The pre-#4851 ad-hoc scanners had no `ObjectAssign` arm, so the
    /// literal's closures were invisible to the max-id scans —
    /// `transform_generators` minted the same FuncId for its synthesized
    /// `__async_step` closure and codegen compiled the arrow body in its
    /// place, silently dropping the await continuation of any caller that
    /// awaited the function directly. The scans now recurse through
    /// `perry_hir::walker::walk_expr_children`, so ANY variant that nests a
    /// closure keeps its ids visible.
    #[test]
    fn object_assign_sources_visible_to_id_scans() {
        let closure = Expr::Closure {
            func_id: 7,
            params: vec![Param {
                id: 9,
                name: "x".to_string(),
                ty: Type::Any,
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            }],
            return_type: Type::Any,
            body: vec![Stmt::Return(Some(Expr::Integer(1)))],
            captures: Vec::new(),
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: false,
        };
        let expr = Expr::ObjectAssign {
            target: Box::new(Expr::LocalGet(3)),
            sources: vec![Expr::New {
                class_name: "__AnonShape_test".to_string(),
                args: vec![closure],
                type_args: Vec::new(),
                byte_offset: 0,
            }],
        };

        let mut max_func: FuncId = 0;
        scan_expr_for_max_func(&expr, &mut max_func);
        assert_eq!(max_func, 7, "closure FuncId inside ObjectAssign sources");

        let mut max_local: LocalId = 0;
        scan_expr_for_max_local(&expr, &mut max_local);
        assert_eq!(
            max_local, 9,
            "closure param LocalId inside ObjectAssign sources"
        );
    }

    fn arrow_field(name: &str, func_id: FuncId) -> ClassField {
        ClassField {
            name: name.to_string(),
            key_expr: None,
            ty: Type::Any,
            init: Some(Expr::Closure {
                func_id,
                params: Vec::new(),
                return_type: Type::Any,
                body: vec![Stmt::Return(Some(Expr::Integer(0)))],
                captures: Vec::new(),
                mutable_captures: Vec::new(),
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: true,
                is_async: false,
                is_generator: false,
                is_strict: false,
            }),
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        }
    }

    fn class_with_fields(name: &str, fields: Vec<ClassField>) -> Class {
        Class {
            id: 1,
            name: name.to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: None,
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields,
            constructor: None,
            methods: Vec::new(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_accessor_names: Vec::new(),
            static_accessor_fn_ids: Vec::new(),
            computed_members: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            aliases: Vec::new(),
            is_nested: false,
        }
    }

    /// #5143: an arrow-function CLASS FIELD initializer (`request = (input)
    /// => ...`) carries a closure whose FuncId is reachable only through
    /// `class.fields[].init` — no method/constructor body holds it. Before
    /// the fix, `compute_max_func_id` skipped class fields, so the
    /// generator/async state-machine transform reused that field's FuncId
    /// for a synthesized step function; at codegen the step body won and the
    /// class field bound the wrong function (Hono's `app.request()` returned
    /// undefined). The scan must see field-initializer closures.
    #[test]
    fn class_field_initializer_closures_visible_to_max_func_id() {
        let mut module = Module::new("test");
        module
            .classes
            .push(class_with_fields("Hono", vec![arrow_field("request", 50)]));
        assert_eq!(
            compute_max_func_id(&module),
            50,
            "field-initializer closure FuncId must be counted"
        );
    }

    /// Companion: static-field initializer closures live in the same FuncId
    /// namespace and must also be visible.
    #[test]
    fn static_field_initializer_closures_visible_to_max_func_id() {
        let mut module = Module::new("test");
        let mut class = class_with_fields("C", Vec::new());
        class.static_fields.push(arrow_field("handler", 73));
        module.classes.push(class);
        assert_eq!(compute_max_func_id(&module), 73);
    }

    /// #5143 (LocalId parallel): a class FIELD-initializer closure's param/body
    /// LocalIds must be visible to `compute_max_local_id` too — they were only
    /// counted for FuncId, so the generator/async transform could mint a
    /// state/done LocalId colliding with a field-init local and corrupt codegen.
    #[test]
    fn class_field_initializer_locals_visible_to_max_local_id() {
        let field = ClassField {
            name: "request".to_string(),
            key_expr: None,
            ty: Type::Any,
            init: Some(Expr::Closure {
                func_id: 3,
                params: vec![Param {
                    id: 91,
                    name: "input".to_string(),
                    ty: Type::Any,
                    default: None,
                    decorators: Vec::new(),
                    is_rest: false,
                    arguments_object: None,
                }],
                return_type: Type::Any,
                body: vec![Stmt::Return(Some(Expr::LocalGet(91)))],
                captures: Vec::new(),
                mutable_captures: Vec::new(),
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: true,
                is_async: false,
                is_generator: false,
                is_strict: false,
            }),
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        };
        let mut module = Module::new("test");
        module.classes.push(class_with_fields("Hono", vec![field]));
        assert_eq!(
            compute_max_local_id(&module),
            91,
            "field-initializer closure param LocalId must be counted"
        );
    }
}
