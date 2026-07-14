use perry_hir::walker::walk_expr_children;
use perry_hir::{Class, Expr, Module, Stmt};
use std::collections::{HashMap, HashSet};

use super::*;

pub fn is_cross_module_safe(body: &[Stmt]) -> bool {
    fn check_expr(expr: &Expr) -> bool {
        match expr {
            // The disqualifying variants — anything tied to a particular
            // module's symbol table.
            Expr::FuncRef(_)
            | Expr::ExternFuncRef { .. }
            | Expr::GlobalGet(_)
            | Expr::GlobalSet(_, _)
            | Expr::NativeModuleRef(_) => false,
            // Closures are out of scope for cross-module inlining: the
            // closure body has its own LocalIds, captures lists, and may
            // reference symbols we can't safely move.
            Expr::Closure { .. } => false,
            // Everything else: descend into all sub-expressions via the
            // central walker.
            other => {
                let mut ok = true;
                walk_expr_children(other, &mut |child| {
                    if !check_expr(child) {
                        ok = false;
                    }
                });
                ok
            }
        }
    }
    fn check_stmt(s: &Stmt) -> bool {
        match s {
            Stmt::Let { init, .. } => init.as_ref().is_none_or(check_expr),
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => check_expr(e),
            Stmt::Return(None) | Stmt::Break | Stmt::Continue => true,
            Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => true,
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expr(condition)
                    && then_branch.iter().all(check_stmt)
                    && else_branch
                        .as_ref()
                        .is_none_or(|eb| eb.iter().all(check_stmt))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                check_expr(condition) && body.iter().all(check_stmt)
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_none_or(|s| check_stmt(s))
                    && condition.as_ref().is_none_or(check_expr)
                    && update.as_ref().is_none_or(check_expr)
                    && body.iter().all(check_stmt)
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                check_expr(discriminant)
                    && cases.iter().all(|c| {
                        c.test.as_ref().is_none_or(check_expr) && c.body.iter().all(check_stmt)
                    })
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                body.iter().all(check_stmt)
                    && catch.as_ref().is_none_or(|c| c.body.iter().all(check_stmt))
                    && finally.as_ref().is_none_or(|f| f.iter().all(check_stmt))
            }
            Stmt::Labeled { body, .. } => check_stmt(body.as_ref()),
            Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => true,
        }
    }
    body.iter().all(check_stmt)
}

/// Harvest inlinable, cross-module-safe methods from `module`. Used by the
/// compile driver to assemble the `extra_methods` map that subsequent modules
/// receive in `inline_functions`. Only methods that pass both `is_inlinable`
/// (the existing per-module gate) and `is_cross_module_safe` (the symbol-
/// frontier gate) make it into the result. Constructors, getters, setters,
/// and static methods are excluded — those have either non-trivial dispatch
/// semantics or a class-tied receiver that cross-module callers can't supply.
/// Harvest content-addressed anon-shape classes (`__AnonShape_<hash>`)
/// from a module. The driver merges these across all prior modules and
/// passes the result to `inline_functions` as `extra_anon_classes` so the
/// destination module gets any class definitions referenced by inlined
/// cross-module method bodies. Hash naming makes dedup-by-name correct
/// (same shape from any module → same name → identical class definition).
pub fn gather_cross_module_anon_classes(module: &Module) -> HashMap<String, &Class> {
    let mut out: HashMap<String, &Class> = HashMap::new();
    for class in &module.classes {
        if class.name.starts_with("__AnonShape_") {
            out.insert(class.name.clone(), class);
        }
    }
    out
}

pub fn gather_cross_module_methods(module: &Module) -> HashMap<(String, String), MethodCandidate> {
    let mut out: HashMap<(String, String), MethodCandidate> = HashMap::new();
    let nonexported = collect_nonexported_class_names(module);
    for class in &module.classes {
        if class.native_extends.is_some() {
            continue;
        }
        for method in &class.methods {
            if !is_inlinable(method) {
                continue;
            }
            if !is_cross_module_safe(&method.body) {
                continue;
            }
            if body_references_class_in_set(&method.body, &nonexported) {
                continue;
            }
            out.insert(
                (class.name.clone(), method.name.clone()),
                MethodCandidate {
                    func: method.clone(),
                    this_param_id: None,
                    method_lookup_safe: method_lookup_is_unshadowed(
                        &module.classes,
                        &class.name,
                        &method.name,
                    ),
                    required_extern_imports: Vec::new(),
                },
            );
        }
    }
    out
}

/// Like `gather_cross_module_methods`, but additionally permits methods that
/// invoke `Expr::ExternFuncRef` — recording each referenced name in
/// `required_extern_imports` so the inline-time safety check can verify the
/// destination module imports the same names before inlining.
///
/// `Expr::FuncRef` (same-module function-id reference) and `Expr::GlobalGet`
/// remain disallowed: function-id and module-globals can't survive a cross-
/// module move at all (the source module's symbol space isn't visible).
/// Closures and `Expr::NativeModuleRef` also remain disallowed.
///
/// The hot motivator here is `World.resolveSetOperation` — its body invokes
/// the imported `getDetailedIdType` (an ExternFuncRef in the World module),
/// which the strict filter rejected. With this looser filter the method
/// becomes a candidate; the inline-time check then permits it iff the
/// destination module also imports `getDetailedIdType`.
pub fn gather_cross_module_methods_with_extern_imports(
    module: &Module,
) -> HashMap<(String, String), MethodCandidate> {
    let mut out: HashMap<(String, String), MethodCandidate> = HashMap::new();
    let nonexported = collect_nonexported_class_names(module);
    // Pre-build a name → resolved_path map from this module's imports so we
    // can resolve each ExternFuncRef in a method body to its source-of-truth.
    // The destination module needs that resolved_path to add the matching
    // Import (the codegen's import_function_prefixes lookup keys on it).
    let mut import_name_to_path: HashMap<String, String> = HashMap::new();
    for imp in &module.imports {
        let Some(path) = imp.resolved_path.clone() else {
            continue;
        };
        for spec in &imp.specifiers {
            if let perry_hir::ImportSpecifier::Named { local, .. } = spec {
                import_name_to_path.insert(local.clone(), path.clone());
            }
        }
    }
    for class in &module.classes {
        if class.native_extends.is_some() {
            continue;
        }
        for method in &class.methods {
            if !is_inlinable(method) {
                continue;
            }
            let mut extern_names: Vec<String> = Vec::new();
            if !is_cross_module_safe_with_externs(&method.body, &mut extern_names) {
                continue;
            }
            // Refs #486: a method body that constructs a non-exported local
            // class (`new InnerPrivate()`) can't be safely inlined into another
            // module — the destination module won't have `InnerPrivate` in its
            // class registry, so `lower_new("InnerPrivate")` falls into the
            // placeholder path that allocates an empty object with class_id=0.
            // Subsequent `inst.method()` dispatch then can't find a vtable
            // entry and falls through to NULL_OBJECT_BYTES. Keep the call as
            // a real cross-module method call (`bl perry_method_<src>__C__m`)
            // so the source module's codegen — which DOES have the class
            // metadata — emits the correct inline-alloc with the right
            // class_id.
            if body_references_class_in_set(&method.body, &nonexported) {
                continue;
            }
            extern_names.sort();
            extern_names.dedup();
            // Resolve each extern name against this module's imports. If
            // any name is unresolvable (it's referenced via ExternFuncRef
            // but doesn't appear as a Named import in this module — could
            // happen for built-ins like `setTimeout` that get
            // ExternFuncRef'd without a corresponding import statement),
            // skip the candidate entirely. The inline-time path needs a
            // concrete source path to copy over.
            let mut required: Vec<(String, String)> = Vec::with_capacity(extern_names.len());
            let mut resolvable = true;
            for name in &extern_names {
                if let Some(p) = import_name_to_path.get(name) {
                    required.push((name.clone(), p.clone()));
                } else {
                    resolvable = false;
                    break;
                }
            }
            if !resolvable {
                continue;
            }
            out.insert(
                (class.name.clone(), method.name.clone()),
                MethodCandidate {
                    func: method.clone(),
                    this_param_id: None,
                    method_lookup_safe: method_lookup_is_unshadowed(
                        &module.classes,
                        &class.name,
                        &method.name,
                    ),
                    required_extern_imports: required,
                },
            );
        }
    }
    out
}

/// Variant of `is_cross_module_safe` that allows `Expr::ExternFuncRef` and
/// records each referenced name into `extern_names`. Used by
/// `gather_cross_module_methods_with_extern_imports`. Same disqualifying
/// rules for FuncRef / GlobalGet / NativeModuleRef / Closure.
pub fn is_cross_module_safe_with_externs(body: &[Stmt], extern_names: &mut Vec<String>) -> bool {
    fn check_expr(expr: &Expr, extern_names: &mut Vec<String>) -> bool {
        match expr {
            Expr::FuncRef(_)
            | Expr::GlobalGet(_)
            | Expr::GlobalSet(_, _)
            | Expr::NativeModuleRef(_) => false,
            Expr::Closure { .. } => false,
            Expr::ExternFuncRef { name, .. } => {
                extern_names.push(name.clone());
                true
            }
            other => {
                let mut ok = true;
                walk_expr_children(other, &mut |child| {
                    if !check_expr(child, extern_names) {
                        ok = false;
                    }
                });
                ok
            }
        }
    }
    fn check_stmt(s: &Stmt, extern_names: &mut Vec<String>) -> bool {
        match s {
            Stmt::Let { init, .. } => init.as_ref().is_none_or(|e| check_expr(e, extern_names)),
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => check_expr(e, extern_names),
            Stmt::Return(None) | Stmt::Break | Stmt::Continue => true,
            Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => true,
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expr(condition, extern_names)
                    && then_branch.iter().all(|s| check_stmt(s, extern_names))
                    && else_branch
                        .as_ref()
                        .is_none_or(|eb| eb.iter().all(|s| check_stmt(s, extern_names)))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                check_expr(condition, extern_names)
                    && body.iter().all(|s| check_stmt(s, extern_names))
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_none_or(|s| check_stmt(s, extern_names))
                    && condition
                        .as_ref()
                        .is_none_or(|e| check_expr(e, extern_names))
                    && update.as_ref().is_none_or(|e| check_expr(e, extern_names))
                    && body.iter().all(|s| check_stmt(s, extern_names))
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                check_expr(discriminant, extern_names)
                    && cases.iter().all(|c| {
                        c.test.as_ref().is_none_or(|e| check_expr(e, extern_names))
                            && c.body.iter().all(|s| check_stmt(s, extern_names))
                    })
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                body.iter().all(|s| check_stmt(s, extern_names))
                    && catch
                        .as_ref()
                        .is_none_or(|c| c.body.iter().all(|s| check_stmt(s, extern_names)))
                    && finally
                        .as_ref()
                        .is_none_or(|f| f.iter().all(|s| check_stmt(s, extern_names)))
            }
            Stmt::Labeled { body, .. } => check_stmt(body.as_ref(), extern_names),
            Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => true,
        }
    }
    body.iter().all(|s| check_stmt(s, extern_names))
}

/// Collect the names of every class declared in `module` that is NOT exported.
/// These are the classes that can't safely cross a module boundary via the
/// inline-method-body copy path: callers in other modules don't see them in
/// their `imported_classes` table, so any `Expr::New { class_name }` /
/// `Expr::ClassRef` / `Expr::StaticFieldGet` / etc. that names one of these
/// classes will lose its class metadata at codegen time. Refs #486.
///
/// The `__AnonShape_*` content-addressed shapes are deliberately INCLUDED in
/// the set despite never being marked `is_exported` — but the inliner already
/// propagates them via `extra_anon_classes` so the destination module
/// synthesizes the same definition. We exclude them here so methods that
/// `new __AnonShape_<hash>()` keep their inlinability.
pub fn collect_nonexported_class_names(module: &Module) -> HashSet<String> {
    let mut set = HashSet::new();
    for c in &module.classes {
        if c.is_exported {
            // Refs #486: even for an EXPORTED class, the inner self-binding
            // alias from `var X = class _X` (recorded in `c.aliases`) is NOT
            // exported under the inner name — only the outer name `X` is
            // visible cross-module. A method body that constructs `new _X()`
            // (e.g. hono `Node.insert` doing `new _Node()` inside an exported
            // `class Node = class _Node`) can't be inlined into a destination
            // module, because the destination only sees `Node` in its
            // `imported_classes` table — `_Node` falls into the
            // `js_object_alloc(0, 0)` placeholder path. Add the alias names
            // to the rejection set so methods that reference them stay as
            // real cross-module method calls.
            for alias in &c.aliases {
                set.insert(alias.clone());
            }
            continue;
        }
        if c.name.starts_with("__AnonShape_") {
            continue;
        }
        set.insert(c.name.clone());
        for alias in &c.aliases {
            set.insert(alias.clone());
        }
    }
    set
}

/// Returns true iff `stmts` references any class whose name is in `set`.
/// Walks every Expr variant that carries a `class_name` string. Used by
/// the cross-module method gathering passes to reject candidates whose
/// body would dangle (or worse: silently fall to a class_id=0 placeholder)
/// after being copied into a destination module.
pub fn body_references_class_in_set(stmts: &[Stmt], set: &HashSet<String>) -> bool {
    fn check_expr(expr: &Expr, set: &HashSet<String>) -> bool {
        match expr {
            Expr::New { class_name, .. }
            | Expr::ClassRef(class_name)
            | Expr::StaticFieldGet { class_name, .. }
            | Expr::StaticFieldSet { class_name, .. }
            | Expr::ClassStaticSymbolSet { class_name, .. }
            | Expr::RegisterClassParentDynamic { class_name, .. }
            | Expr::RegisterClassStaticSymbol { class_name, .. }
            | Expr::StaticMethodCall { class_name, .. }
                if set.contains(class_name) =>
            {
                return true;
            }
            Expr::ClassExprFresh { template, .. } if set.contains(template) => {
                return true;
            }
            _ => {}
        }
        let mut hit = false;
        walk_expr_children(expr, &mut |child| {
            if check_expr(child, set) {
                hit = true;
            }
        });
        hit
    }
    fn check_stmt(s: &Stmt, set: &HashSet<String>) -> bool {
        match s {
            Stmt::Let { init, .. } => init.as_ref().is_some_and(|e| check_expr(e, set)),
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => check_expr(e, set),
            Stmt::Return(None) | Stmt::Break | Stmt::Continue => false,
            Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => false,
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                check_expr(condition, set)
                    || then_branch.iter().any(|s| check_stmt(s, set))
                    || else_branch
                        .as_ref()
                        .is_some_and(|eb| eb.iter().any(|s| check_stmt(s, set)))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                check_expr(condition, set) || body.iter().any(|s| check_stmt(s, set))
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_some_and(|s| check_stmt(s, set))
                    || condition.as_ref().is_some_and(|e| check_expr(e, set))
                    || update.as_ref().is_some_and(|e| check_expr(e, set))
                    || body.iter().any(|s| check_stmt(s, set))
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                check_expr(discriminant, set)
                    || cases.iter().any(|c| {
                        c.test.as_ref().is_some_and(|e| check_expr(e, set))
                            || c.body.iter().any(|s| check_stmt(s, set))
                    })
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                body.iter().any(|s| check_stmt(s, set))
                    || catch
                        .as_ref()
                        .is_some_and(|c| c.body.iter().any(|s| check_stmt(s, set)))
                    || finally
                        .as_ref()
                        .is_some_and(|f| f.iter().any(|s| check_stmt(s, set)))
            }
            Stmt::Labeled { body, .. } => check_stmt(body.as_ref(), set),
            Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) => false,
        }
    }
    stmts.iter().any(|s| check_stmt(s, set))
}
