//! Issue #100: helpers for compile-time-resolved dynamic `import()`.
//!
//! Two responsibilities:
//!
//! 1. [`resolve_import_path`] — const-folds the path argument of a
//!    dynamic `import()` to a finite set of module sources. The
//!    supported subset is documented inline; anything outside it
//!    returns [`Resolution::Unresolved`] with a human-readable reason
//!    so the driver can raise a structured compile error.
//!
//! 2. [`detect_top_level_await`] — sets `Module.has_top_level_await`
//!    by scanning `module.init` for any `Expr::Await` outside a
//!    function/closure body. Drives the deferred-import dispatch to
//!    chain the init promise.
//!
//! Neither helper performs filesystem I/O — path resolution to a
//! `resolved_path` is the driver's job (it owns the module resolver).
//! Here we only fold the JS-level path *string*.

use crate::ir::{BinaryOp, Export, Expr, Function, Module, Stmt};
use crate::walker::{walk_expr_children, walk_expr_children_mut};
use std::collections::HashSet;

/// Hard cap on the number of paths a single `import()` site can resolve
/// to. Over-cap produces a compile error per D2 (issue #100).
pub const DYNAMIC_IMPORT_PATH_CAP: usize = 64;

/// Walk every expression in `module` (init statements, top-level functions,
/// class constructors/methods/getters/setters, field initializers, etc.)
/// and invoke `f` with each `&mut Expr::DynamicImport` node found.
///
/// Used by the driver to run [`resolve_import_path`] over every dynamic
/// import site in a freshly lowered module so it can register the
/// resolved targets in the import graph and stamp `paths` on each node.
pub fn for_each_dynamic_import_mut<F: FnMut(&mut Expr)>(module: &mut Module, f: &mut F) {
    for stmt in &mut module.init {
        visit_stmt_for_dyn_imports(stmt, f);
    }
    for func in &mut module.functions {
        visit_function_for_dyn_imports(func, f);
    }
    for cls in &mut module.classes {
        if let Some(ctor) = &mut cls.constructor {
            visit_function_for_dyn_imports(ctor, f);
        }
        for m in &mut cls.methods {
            visit_function_for_dyn_imports(m, f);
        }
        for (_, g) in &mut cls.getters {
            visit_function_for_dyn_imports(g, f);
        }
        for (_, s) in &mut cls.setters {
            visit_function_for_dyn_imports(s, f);
        }
        for m in &mut cls.static_methods {
            visit_function_for_dyn_imports(m, f);
        }
        for field in &mut cls.fields {
            if let Some(init) = &mut field.init {
                visit_expr_for_dyn_imports(init, f);
            }
        }
        for field in &mut cls.static_fields {
            if let Some(init) = &mut field.init {
                visit_expr_for_dyn_imports(init, f);
            }
        }
    }
    for global in &mut module.globals {
        if let Some(init) = &mut global.init {
            visit_expr_for_dyn_imports(init, f);
        }
    }
}

fn visit_function_for_dyn_imports<F: FnMut(&mut Expr)>(func: &mut Function, f: &mut F) {
    for stmt in &mut func.body {
        visit_stmt_for_dyn_imports(stmt, f);
    }
    for param in &mut func.params {
        if let Some(default) = &mut param.default {
            visit_expr_for_dyn_imports(default, f);
        }
    }
}

fn visit_stmt_for_dyn_imports<F: FnMut(&mut Expr)>(stmt: &mut Stmt, f: &mut F) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                visit_expr_for_dyn_imports(e, f);
            }
        }
        Stmt::Expr(e) => visit_expr_for_dyn_imports(e, f),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                visit_expr_for_dyn_imports(e, f);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expr_for_dyn_imports(condition, f);
            for s in then_branch {
                visit_stmt_for_dyn_imports(s, f);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    visit_stmt_for_dyn_imports(s, f);
                }
            }
        }
        Stmt::While { condition, body } => {
            visit_expr_for_dyn_imports(condition, f);
            for s in body {
                visit_stmt_for_dyn_imports(s, f);
            }
        }
        Stmt::DoWhile { body, condition } => {
            for s in body {
                visit_stmt_for_dyn_imports(s, f);
            }
            visit_expr_for_dyn_imports(condition, f);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                visit_stmt_for_dyn_imports(i, f);
            }
            if let Some(c) = condition {
                visit_expr_for_dyn_imports(c, f);
            }
            if let Some(u) = update {
                visit_expr_for_dyn_imports(u, f);
            }
            for s in body {
                visit_stmt_for_dyn_imports(s, f);
            }
        }
        Stmt::Labeled { body, .. } => visit_stmt_for_dyn_imports(body, f),
        Stmt::Throw(e) => visit_expr_for_dyn_imports(e, f),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                visit_stmt_for_dyn_imports(s, f);
            }
            if let Some(c) = catch {
                for s in &mut c.body {
                    visit_stmt_for_dyn_imports(s, f);
                }
            }
            if let Some(fb) = finally {
                for s in fb {
                    visit_stmt_for_dyn_imports(s, f);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            visit_expr_for_dyn_imports(discriminant, f);
            for c in cases {
                if let Some(t) = &mut c.test {
                    visit_expr_for_dyn_imports(t, f);
                }
                for s in &mut c.body {
                    visit_stmt_for_dyn_imports(s, f);
                }
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_) => {}
    }
}

fn visit_expr_for_dyn_imports<F: FnMut(&mut Expr)>(expr: &mut Expr, f: &mut F) {
    if matches!(expr, Expr::DynamicImport { .. }) {
        f(expr);
        // After f mutates the node, still descend into the (possibly
        // unchanged) `arg` so nested dynamic imports are visited.
        if let Expr::DynamicImport { arg, .. } = expr {
            visit_expr_for_dyn_imports(arg, f);
        }
        return;
    }
    // Closure bodies — descend manually (the walker intentionally
    // doesn't).
    if let Expr::Closure { body, .. } = expr {
        for s in body {
            visit_stmt_for_dyn_imports(s, f);
        }
    }
    walk_expr_children_mut(expr, &mut |child| visit_expr_for_dyn_imports(child, f));
}

/// The result of const-folding a dynamic `import()` path argument.
#[derive(Debug, Clone)]
pub enum Resolution {
    /// The argument resolves to this non-empty, bounded set of module
    /// sources. The driver registers each as an import edge.
    Set(Vec<String>),
    /// The argument cannot be statically resolved. The driver should
    /// raise a compile error citing this reason.
    Unresolved(String),
}

impl Resolution {
    fn merge(self, other: Resolution) -> Resolution {
        match (self, other) {
            (Resolution::Set(mut a), Resolution::Set(b)) => {
                for p in b {
                    if !a.contains(&p) {
                        a.push(p);
                    }
                }
                Resolution::Set(a)
            }
            (Resolution::Unresolved(r), _) | (_, Resolution::Unresolved(r)) => {
                Resolution::Unresolved(r)
            }
        }
    }
}

/// Issue #100: one entry in the flat-export list of a module that may
/// be the target of a dynamic `import()`. Returned by [`flatten_exports`]
/// after resolving `ReExport` / `ExportAll` / `NamespaceReExport` through
/// the module graph.
///
/// The codegen consumes this list to populate the module's
/// `__perry_ns_<prefix>` global at the end of `__perry_init_<prefix>`.
/// Each entry maps one exported name (as the consumer sees it) to the
/// module + local binding that actually holds the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatExport {
    /// The key as seen by the consumer of `await import("...")`.
    pub name: String,
    /// Module that owns the binding holding the value. For local exports
    /// this is the same module passed to `flatten_exports`; for re-
    /// exports it's the upstream module the value transitively came
    /// from.
    pub source_module: String,
    /// The local name in `source_module` that holds the value.
    pub source_local: String,
    /// For `NamespaceReExport` — when `Some(nested_source)`, this entry
    /// represents `name → namespace_of(nested_source)`. Codegen emits a
    /// nested `js_create_namespace` call sourced from that module's own
    /// `__perry_ns_<prefix>`. Otherwise `None` (the typical case).
    pub nested_namespace_of: Option<String>,
}

/// Issue #100: resolve a module's exports — flattening `ExportAll`,
/// `ReExport`, and `NamespaceReExport` through the import graph — into
/// a flat list suitable for namespace materialization.
///
/// `modules` is a lookup of every module by `Module::name` (the same
/// string used in `Import::source` / `Export::*::source` resolution
/// keys). The caller is responsible for resolving module specifiers
/// (e.g. `"./foo.ts"` vs `Module::name`) up-front and keying `modules`
/// consistently — both `Export::ReExport::source` strings and
/// `Module::name` must use the same form.
///
/// Cycle-safe: a `visited` set tracks modules we've already descended
/// into so an `export * from` cycle terminates without infinite
/// recursion. The first encounter wins (depth-first).
///
/// Returns entries in declaration order with later entries overriding
/// earlier ones on duplicate names (matches JS semantics for
/// `export * from`).
pub fn flatten_exports<'a, F>(target_name: &str, lookup: &F) -> Vec<FlatExport>
where
    F: Fn(&str) -> Option<&'a Module>,
{
    let mut out: Vec<FlatExport> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    flatten_into(target_name, lookup, &mut out, &mut visited);
    // Preserve last-writer-wins on duplicate names while keeping insertion order.
    let mut seen: HashSet<String> = HashSet::new();
    let mut dedup: Vec<FlatExport> = Vec::new();
    for entry in out.into_iter().rev() {
        if seen.insert(entry.name.clone()) {
            dedup.push(entry);
        }
    }
    dedup.reverse();
    dedup
}

fn flatten_into<'a, F>(
    module_name: &str,
    lookup: &F,
    out: &mut Vec<FlatExport>,
    visited: &mut HashSet<String>,
) where
    F: Fn(&str) -> Option<&'a Module>,
{
    if !visited.insert(module_name.to_string()) {
        return;
    }
    let module = match lookup(module_name) {
        Some(m) => m,
        None => return,
    };
    for export in &module.exports {
        match export {
            Export::Named { local, exported } => {
                out.push(FlatExport {
                    name: exported.clone(),
                    source_module: module_name.to_string(),
                    source_local: local.clone(),
                    nested_namespace_of: None,
                });
            }
            Export::ReExport {
                source,
                imported,
                exported,
            } => {
                // The value lives in `source`; if `source` re-exports it
                // again, we want to follow that chain so codegen reaches
                // the ULTIMATE owner. But for the MVP we surface one hop
                // — the cross-module access pattern works regardless of
                // how many hops away the value's defining module is, as
                // long as we name the directly-importing source.
                out.push(FlatExport {
                    name: exported.clone(),
                    source_module: source.clone(),
                    source_local: imported.clone(),
                    nested_namespace_of: None,
                });
            }
            Export::ExportAll { source } => {
                // Recursively flatten the source's exports into ours.
                // Cycle-safe via `visited`; depth-first so a closer
                // re-exporter wins on name collision (matches the
                // dedup pass above).
                flatten_into(source, lookup, out, visited);
            }
            Export::NamespaceReExport { source, name } => {
                out.push(FlatExport {
                    name: name.clone(),
                    source_module: source.clone(),
                    source_local: String::new(),
                    nested_namespace_of: Some(source.clone()),
                });
            }
        }
    }
}

/// Issue #100 / #1725 / #1674: collect every `Stmt::Let { init: Some(_), .. }`
/// reachable in the module into a `local_id → init_expr` map — the module-init
/// body, every function / method / constructor body, and (descending) nested
/// closure bodies.
///
/// Pre-#1725 this collected ONLY top-level module consts, on the assumption that
/// a dynamic `import()` argument is always evaluated in module-init scope. That
/// is wrong: `import()` can sit inside a function. hono's
/// `hono/dist/utils/color.js` does
/// ```js
/// async function getColorEnabledAsync() {
///   const cfWorkers = "cloudflare:workers";
///   try { return "NO_COLOR" in ((await import(cfWorkers)).env ?? {}); } catch { return false; }
/// }
/// ```
/// — a function-local `const` string literal used as the specifier (wrapped in
/// the optional-dep `try/catch` idiom). At this HIR stage closures are still
/// inline and capture by the *original* `LocalId`, and `for_each_dynamic_import_mut`
/// descends into closure bodies, so a const declared in any enclosing scope
/// resolves at the import site. `LocalId`s are module-unique, so a single flat
/// id→init map across all scopes is unambiguous.
///
/// Both `const` and `let`/`var` single-init bindings participate, but any
/// binding that is *reassigned* anywhere (a later `LocalSet`) is excluded by the
/// mutation scan below — so the effective constraint is the spec's "single SSA
/// def to a resolvable expression" without a full SSA pass. `const` guarantees
/// this by construction; a `let p = <init>` that is never written again is
/// single-assignment in practice and resolves identically (#1674). A genuinely
/// mutated binding falls back to Unresolved.
pub fn collect_module_const_locals(module: &Module) -> std::collections::HashMap<u32, Expr> {
    use std::collections::HashMap;
    let mut consts: HashMap<u32, Expr> = HashMap::new();

    // Gather every function body and standalone init expression reachable in
    // the module — the SAME scope set `for_each_dynamic_import_mut` walks
    // (top-level, functions, class ctor/methods/getters/setters/static-methods,
    // field + global initializers). Collecting consts from all of them means a
    // const in scope at *any* dynamic-import site resolves, regardless of where
    // the `import()` sits (#1725).
    let mut funcs: Vec<&Function> = module.functions.iter().collect();
    let mut init_exprs: Vec<&Expr> = Vec::new();
    for cls in &module.classes {
        if let Some(ctor) = &cls.constructor {
            funcs.push(ctor);
        }
        funcs.extend(cls.methods.iter());
        funcs.extend(cls.getters.iter().map(|(_, f)| f));
        funcs.extend(cls.setters.iter().map(|(_, f)| f));
        funcs.extend(cls.static_methods.iter());
        for field in cls.fields.iter().chain(cls.static_fields.iter()) {
            if let Some(init) = &field.init {
                init_exprs.push(init);
            }
        }
    }
    for g in &module.globals {
        if let Some(init) = &g.init {
            init_exprs.push(init);
        }
    }

    for stmt in &module.init {
        collect_const_locals_stmt(stmt, &mut consts);
    }
    for func in &funcs {
        for s in &func.body {
            collect_const_locals_stmt(s, &mut consts);
        }
        for p in &func.params {
            if let Some(d) = &p.default {
                collect_const_locals_expr(d, &mut consts);
            }
        }
    }
    for e in &init_exprs {
        collect_const_locals_expr(e, &mut consts);
    }

    // Any later mutation invalidates the entry — walk the same scope set
    // (descending into closures) and remove ids that get reassigned.
    let mut mutated: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for stmt in &module.init {
        scan_mutations_stmt(stmt, &mut mutated);
    }
    for func in &funcs {
        for s in &func.body {
            scan_mutations_stmt(s, &mut mutated);
        }
        for p in &func.params {
            if let Some(d) = &p.default {
                scan_mutations_expr(d, &mut mutated);
            }
        }
    }
    for e in &init_exprs {
        scan_mutations_expr(e, &mut mutated);
    }

    for id in mutated {
        consts.remove(&id);
    }
    consts
}

/// Collect `const x = <init>` bindings reachable from `stmt` into `out`,
/// recursing through nested blocks (#1725). Mirrors `scan_mutations_stmt`'s
/// traversal and additionally descends into closure bodies via
/// `collect_const_locals_expr`.
fn collect_const_locals_stmt(stmt: &Stmt, out: &mut std::collections::HashMap<u32, Expr>) {
    match stmt {
        Stmt::Let {
            id, init: Some(e), ..
        } => {
            // #1674: collect both `const` and never-reassigned `let`/`var`
            // bindings (regardless of the `mutable` flag). A `let p = <expr>`
            // that is never written again is single-assignment in practice —
            // `collect_module_const_locals`'s mutation scan removes any id that
            // later receives a `LocalSet`, so a genuinely reassigned binding
            // still falls back to Unresolved. This lets a path bound to a
            // resolvable init resolve, e.g.
            // `let p = cond ? './a.ts' : './b.ts'; await import(p)`.
            out.insert(*id, e.clone());
            collect_const_locals_expr(e, out);
        }
        Stmt::Let { init: None, .. } => {}
        Stmt::Expr(e) => collect_const_locals_expr(e, out),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                collect_const_locals_expr(e, out);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_const_locals_expr(condition, out);
            for s in then_branch {
                collect_const_locals_stmt(s, out);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    collect_const_locals_stmt(s, out);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_const_locals_expr(condition, out);
            for s in body {
                collect_const_locals_stmt(s, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                collect_const_locals_stmt(i, out);
            }
            if let Some(c) = condition {
                collect_const_locals_expr(c, out);
            }
            if let Some(u) = update {
                collect_const_locals_expr(u, out);
            }
            for s in body {
                collect_const_locals_stmt(s, out);
            }
        }
        Stmt::Labeled { body, .. } => collect_const_locals_stmt(body, out),
        Stmt::Throw(e) => collect_const_locals_expr(e, out),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_const_locals_stmt(s, out);
            }
            if let Some(c) = catch {
                for s in &c.body {
                    collect_const_locals_stmt(s, out);
                }
            }
            if let Some(fb) = finally {
                for s in fb {
                    collect_const_locals_stmt(s, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_const_locals_expr(discriminant, out);
            for c in cases {
                if let Some(t) = &c.test {
                    collect_const_locals_expr(t, out);
                }
                for s in &c.body {
                    collect_const_locals_stmt(s, out);
                }
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_) => {}
    }
}

/// Descend into an expression collecting const locals declared inside closure
/// bodies (`walk_expr_children` deliberately skips closure bodies, so handle
/// them explicitly). #1725.
fn collect_const_locals_expr(expr: &Expr, out: &mut std::collections::HashMap<u32, Expr>) {
    if let Expr::Closure { body, .. } = expr {
        for s in body {
            collect_const_locals_stmt(s, out);
        }
    }
    walk_expr_children(expr, &mut |child| collect_const_locals_expr(child, out));
}

fn scan_mutations_stmt(stmt: &Stmt, out: &mut std::collections::HashSet<u32>) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                scan_mutations_expr(e, out);
            }
        }
        Stmt::Expr(e) => scan_mutations_expr(e, out),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                scan_mutations_expr(e, out);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            scan_mutations_expr(condition, out);
            for s in then_branch {
                scan_mutations_stmt(s, out);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    scan_mutations_stmt(s, out);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            scan_mutations_expr(condition, out);
            for s in body {
                scan_mutations_stmt(s, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                scan_mutations_stmt(i, out);
            }
            if let Some(c) = condition {
                scan_mutations_expr(c, out);
            }
            if let Some(u) = update {
                scan_mutations_expr(u, out);
            }
            for s in body {
                scan_mutations_stmt(s, out);
            }
        }
        Stmt::Labeled { body, .. } => scan_mutations_stmt(body, out),
        Stmt::Throw(e) => scan_mutations_expr(e, out),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                scan_mutations_stmt(s, out);
            }
            if let Some(c) = catch {
                for s in &c.body {
                    scan_mutations_stmt(s, out);
                }
            }
            if let Some(fb) = finally {
                for s in fb {
                    scan_mutations_stmt(s, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            scan_mutations_expr(discriminant, out);
            for c in cases {
                if let Some(t) = &c.test {
                    scan_mutations_expr(t, out);
                }
                for s in &c.body {
                    scan_mutations_stmt(s, out);
                }
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_) => {}
    }
}

fn scan_mutations_expr(expr: &Expr, out: &mut std::collections::HashSet<u32>) {
    match expr {
        Expr::LocalSet(id, _) => {
            out.insert(*id);
        }
        Expr::Update { id, .. } => {
            out.insert(*id);
        }
        _ => {}
    }
    // `walk_expr_children` deliberately skips closure bodies; descend manually
    // so a const reassigned inside a nested closure still invalidates the
    // entry now that function/closure-scope consts are collected (#1725).
    if let Expr::Closure { body, .. } = expr {
        for s in body {
            scan_mutations_stmt(s, out);
        }
    }
    walk_expr_children(expr, &mut |child| scan_mutations_expr(child, out));
}

/// Const-fold a dynamic `import()` path argument.
///
/// Supported forms (D1, issue #100):
///   - String literal:                    `import('./foo.ts')`
///   - Ternary of two resolvable args:    `import(cond ? a : b)`
///   - Template literal:                  ``import(`./locale_${lang}.ts`)``
///     (expanded to Cartesian product of every interpolation's
///     resolvable set; over the path cap surfaces as Unresolved with a
///     clear message via the caller).
///   - Module-level `const` local:        `const x = './foo.ts'; await
///     import(x)` — resolved transitively against the `consts` map
///     built by [`collect_module_const_locals`]. Inside the local's
///     initializer, the same const-folding rules apply, so consts can
///     reference other consts.
///   - Parenthesized / `as` / `satisfies` wrapper: not represented in
///     HIR (already elided during lowering).
///
/// The `consts` map is `LocalId → init_expr` for every module-level
/// non-mutated `const`. Pass an empty map to disable the local-tracking
/// branch (matches the original signature semantics).
pub fn resolve_import_path(arg: &Expr) -> Resolution {
    let empty: std::collections::HashMap<u32, Expr> = std::collections::HashMap::new();
    resolve_import_path_with_consts(arg, &empty, &mut std::collections::HashSet::new())
}

/// Like [`resolve_import_path`] but threaded through a `consts` map so
/// const-propagated locals can resolve transitively. `visiting` is a
/// per-call cycle-breaker — a const initializer that references its
/// own id (impossible in well-formed TS, but defensive) returns
/// Unresolved instead of recursing infinitely.
/// #1674 sub-part B (glob): when a template-literal specifier has a fixed,
/// relative, directory-anchored `prefix`, a fixed `suffix`, and a
/// non-statically-resolvable middle (`import(`./plugins/${name}.ts`)`),
/// return `(prefix, suffix)` so the driver can glob `<prefix>*<suffix>`
/// against the importing module's directory and enumerate the candidates.
///
/// Returns `None` for anything that isn't this shape — fully-resolvable
/// templates (handled by [`resolve_import_path_with_consts`]) and patterns
/// with no fixed, directory-bearing prefix (too broad to glob safely). The
/// resolver itself performs no filesystem I/O; the driver owns the readdir.
pub fn dynamic_import_glob_pattern(
    arg: &Expr,
    consts: &std::collections::HashMap<u32, Expr>,
) -> Option<(String, String)> {
    // Only template-literal concatenations (`Binary(Add, …)`) can glob.
    if !matches!(
        arg,
        Expr::Binary {
            op: BinaryOp::Add,
            ..
        }
    ) {
        return None;
    }
    let mut parts: Vec<&Expr> = Vec::new();
    flatten_concat(arg, &mut parts);
    if parts.len() < 2 {
        return None;
    }

    // A part resolves to a single fixed string, or it doesn't (wildcard).
    let single = |p: &Expr| -> Option<String> {
        let mut visiting = std::collections::HashSet::new();
        match resolve_import_path_with_consts(p, consts, &mut visiting) {
            Resolution::Set(v) if v.len() == 1 => Some(v.into_iter().next().unwrap()),
            _ => None,
        }
    };

    // Leading fixed parts → prefix.
    let mut prefix = String::new();
    let mut i = 0;
    while i < parts.len() {
        match single(parts[i]) {
            Some(s) => {
                prefix.push_str(&s);
                i += 1;
            }
            None => break,
        }
    }
    // Trailing fixed parts → suffix.
    let mut suffix = String::new();
    let mut j = parts.len();
    while j > i {
        match single(parts[j - 1]) {
            Some(s) => {
                suffix.insert_str(0, &s);
                j -= 1;
            }
            None => break,
        }
    }
    // Need at least one non-fixed (wildcard) part between prefix and suffix.
    if i >= j {
        return None;
    }
    // The prefix must be a relative specifier with a directory component so
    // the glob is scoped to one folder (never the whole project / node_modules).
    if !(prefix.starts_with("./") || prefix.starts_with("../")) || !prefix.contains('/') {
        return None;
    }
    Some((prefix, suffix))
}

pub fn resolve_import_path_with_consts(
    arg: &Expr,
    consts: &std::collections::HashMap<u32, Expr>,
    visiting: &mut std::collections::HashSet<u32>,
) -> Resolution {
    match arg {
        Expr::String(s) => Resolution::Set(vec![s.clone()]),
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            let a = resolve_import_path_with_consts(then_expr, consts, visiting);
            let b = resolve_import_path_with_consts(else_expr, consts, visiting);
            a.merge(b)
        }
        // Template literal — desugared to `Binary(Add, ...)` chains by
        // `expr_misc::lower_tpl`. We re-flatten the chain into the
        // ordered list of parts, then take the Cartesian product of
        // each part's resolved set. Cap-enforcement happens at the
        // call site (`collect_modules`) which already gates on
        // `DYNAMIC_IMPORT_PATH_CAP`; doing it again here would
        // duplicate the error message.
        Expr::Binary {
            op: BinaryOp::Add, ..
        } => {
            let mut parts: Vec<&Expr> = Vec::new();
            flatten_concat(arg, &mut parts);
            // Each part resolves to a finite set of strings; the result
            // is the Cartesian product. Short-circuit if any part is
            // Unresolved.
            let mut sets: Vec<Vec<String>> = Vec::with_capacity(parts.len());
            for p in &parts {
                match resolve_import_path_with_consts(p, consts, visiting) {
                    Resolution::Set(v) => sets.push(v),
                    Resolution::Unresolved(r) => return Resolution::Unresolved(r),
                }
            }
            // Cartesian product.
            let mut acc: Vec<String> = vec![String::new()];
            for part_set in sets {
                let mut next: Vec<String> = Vec::with_capacity(acc.len() * part_set.len());
                for prefix in &acc {
                    for suffix in &part_set {
                        next.push(format!("{}{}", prefix, suffix));
                    }
                }
                acc = next;
                // Bail early if cardinality exceeds the cap — the
                // caller's gate also catches this but reporting it
                // here avoids worst-case quadratic growth.
                if acc.len() > DYNAMIC_IMPORT_PATH_CAP {
                    return Resolution::Set(acc); // caller emits cap error
                }
            }
            // Dedup while preserving first-occurrence order.
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            acc.retain(|s| seen.insert(s.clone()));
            Resolution::Set(acc)
        }
        // Module-level `const x = '...'` reference. Recurse into the
        // const's init expression; cycle-break via `visiting`.
        Expr::LocalGet(id) => {
            if !visiting.insert(*id) {
                return Resolution::Unresolved(
                    "circular const reference in path argument".to_string(),
                );
            }
            let resolved = if let Some(init) = consts.get(id) {
                resolve_import_path_with_consts(init, consts, visiting)
            } else {
                Resolution::Unresolved(
                    "path argument references a binding that is not statically \
                     resolvable to a literal (supported: string literals, ternaries, \
                     template literals over resolvable locals, and `const`/never-\
                     reassigned `let` bindings initialized to a resolvable value; a \
                     binding reassigned anywhere falls back here)"
                        .to_string(),
                )
            };
            visiting.remove(id);
            resolved
        }
        _ => Resolution::Unresolved(
            "path argument is not statically resolvable (supported: string literals, \
             ternaries of resolvable arms, template literals with const-local \
             interpolations, and references to module-level const string locals)"
                .to_string(),
        ),
    }
}

/// Flatten a left-leaning `Add` chain — produced by
/// `expr_misc::lower_tpl` for a template literal — into the ordered
/// list of leaf parts. e.g. `(("./locale_" + lang) + ".ts")` flattens
/// to `["./locale_", lang, ".ts"]`. Non-`Add` nodes are leaves.
fn flatten_concat<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
    if let Expr::Binary {
        op: BinaryOp::Add,
        left,
        right,
    } = expr
    {
        flatten_concat(left, out);
        flatten_concat(right, out);
    } else {
        out.push(expr);
    }
}

/// Scan `module.init` for an `await` expression outside any function/
/// closure body and set `module.has_top_level_await` accordingly.
///
/// Idempotent — safe to call multiple times. Closure bodies are NOT
/// descended into because awaits inside them belong to the closure's
/// own async scope, not the module's top level.
pub fn detect_top_level_await(module: &mut Module) {
    let mut found = false;
    for stmt in &module.init {
        if stmt_has_top_level_await(stmt) {
            found = true;
            break;
        }
    }
    module.has_top_level_await = found;
}

fn stmt_has_top_level_await(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Let { init, .. } => init.as_ref().is_some_and(expr_has_top_level_await),
        Stmt::Expr(e) => expr_has_top_level_await(e),
        Stmt::Return(opt) => opt.as_ref().is_some_and(expr_has_top_level_await),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_has_top_level_await(condition)
                || then_branch.iter().any(stmt_has_top_level_await)
                || else_branch
                    .as_ref()
                    .is_some_and(|b| b.iter().any(stmt_has_top_level_await))
        }
        Stmt::While { condition, body } => {
            expr_has_top_level_await(condition) || body.iter().any(stmt_has_top_level_await)
        }
        Stmt::DoWhile { body, condition } => {
            body.iter().any(stmt_has_top_level_await) || expr_has_top_level_await(condition)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_deref().is_some_and(stmt_has_top_level_await)
                || condition.as_ref().is_some_and(expr_has_top_level_await)
                || update.as_ref().is_some_and(expr_has_top_level_await)
                || body.iter().any(stmt_has_top_level_await)
        }
        Stmt::Labeled { body, .. } => stmt_has_top_level_await(body),
        Stmt::Throw(e) => expr_has_top_level_await(e),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().any(stmt_has_top_level_await)
                || catch
                    .as_ref()
                    .is_some_and(|c| c.body.iter().any(stmt_has_top_level_await))
                || finally
                    .as_ref()
                    .is_some_and(|f| f.iter().any(stmt_has_top_level_await))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr_has_top_level_await(discriminant)
                || cases.iter().any(|c| {
                    c.test.as_ref().is_some_and(expr_has_top_level_await)
                        || c.body.iter().any(stmt_has_top_level_await)
                })
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_) => false,
    }
}

fn expr_has_top_level_await(expr: &Expr) -> bool {
    // The walker's `Closure` arm intentionally does NOT descend into the
    // closure body, which is exactly the semantics we need: an `await`
    // inside a nested closure/function belongs to that function's scope,
    // not the module's top level.
    if matches!(expr, Expr::Await(_)) {
        return true;
    }
    let mut found = false;
    walk_expr_children(expr, &mut |child| {
        if !found && expr_has_top_level_await(child) {
            found = true;
        }
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Module;
    use perry_types::Type;

    #[test]
    fn resolve_string_literal() {
        let r = resolve_import_path(&Expr::String("./foo.ts".into()));
        match r {
            Resolution::Set(v) => assert_eq!(v, vec!["./foo.ts"]),
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn resolve_ternary_of_literals() {
        let r = resolve_import_path(&Expr::Conditional {
            condition: Box::new(Expr::Bool(true)),
            then_expr: Box::new(Expr::String("./a.ts".into())),
            else_expr: Box::new(Expr::String("./b.ts".into())),
        });
        match r {
            Resolution::Set(v) => {
                assert_eq!(v.len(), 2);
                assert!(v.contains(&"./a.ts".to_string()));
                assert!(v.contains(&"./b.ts".to_string()));
            }
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn resolve_ternary_dedupes() {
        let r = resolve_import_path(&Expr::Conditional {
            condition: Box::new(Expr::Bool(true)),
            then_expr: Box::new(Expr::String("./a.ts".into())),
            else_expr: Box::new(Expr::String("./a.ts".into())),
        });
        match r {
            Resolution::Set(v) => assert_eq!(v, vec!["./a.ts"]),
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn resolve_unresolvable_local() {
        let r = resolve_import_path(&Expr::LocalGet(0));
        assert!(matches!(r, Resolution::Unresolved(_)));
    }

    #[test]
    fn tla_detects_module_init_await() {
        let mut m = Module::new("t");
        m.init
            .push(Stmt::Expr(Expr::Await(Box::new(Expr::Undefined))));
        detect_top_level_await(&mut m);
        assert!(m.has_top_level_await);
    }

    #[test]
    fn resolve_template_literal_with_const_local() {
        // Simulate the HIR shape produced by `lower_tpl` for
        // `./locale_${lang}.ts` where lang is a module-level const.
        // The Add chain is `("./locale_" + lang) + ".ts"`.
        let arg = Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::String("./locale_".into())),
                right: Box::new(Expr::LocalGet(7)),
            }),
            right: Box::new(Expr::String(".ts".into())),
        };
        let mut consts = std::collections::HashMap::new();
        consts.insert(7u32, Expr::String("es".into()));
        let mut visiting = std::collections::HashSet::new();
        let r = resolve_import_path_with_consts(&arg, &consts, &mut visiting);
        match r {
            Resolution::Set(v) => assert_eq!(v, vec!["./locale_es.ts"]),
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn resolve_template_literal_with_ternary_interpolation() {
        // `./locale_${cond ? 'en' : 'es'}.ts` — Cartesian product.
        let interp = Expr::Conditional {
            condition: Box::new(Expr::Bool(true)),
            then_expr: Box::new(Expr::String("en".into())),
            else_expr: Box::new(Expr::String("es".into())),
        };
        let arg = Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::String("./locale_".into())),
                right: Box::new(interp),
            }),
            right: Box::new(Expr::String(".ts".into())),
        };
        let consts = std::collections::HashMap::new();
        let mut visiting = std::collections::HashSet::new();
        let r = resolve_import_path_with_consts(&arg, &consts, &mut visiting);
        match r {
            Resolution::Set(v) => {
                assert_eq!(v.len(), 2);
                assert!(v.contains(&"./locale_en.ts".to_string()));
                assert!(v.contains(&"./locale_es.ts".to_string()));
            }
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn resolve_local_const_propagation() {
        // `const p = './foo.ts'; import(p)`
        let arg = Expr::LocalGet(3);
        let mut consts = std::collections::HashMap::new();
        consts.insert(3u32, Expr::String("./foo.ts".into()));
        let mut visiting = std::collections::HashSet::new();
        let r = resolve_import_path_with_consts(&arg, &consts, &mut visiting);
        match r {
            Resolution::Set(v) => assert_eq!(v, vec!["./foo.ts"]),
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn resolve_unresolved_param_local() {
        // `function f(p) { import(p) }` — p isn't in the const map.
        let arg = Expr::LocalGet(42);
        let consts = std::collections::HashMap::new();
        let mut visiting = std::collections::HashSet::new();
        let r = resolve_import_path_with_consts(&arg, &consts, &mut visiting);
        assert!(matches!(r, Resolution::Unresolved(_)));
    }

    #[test]
    fn collect_consts_skips_mutated() {
        let mut m = Module::new("t");
        m.init.push(Stmt::Let {
            id: 1,
            name: "stable".into(),
            ty: perry_types::Type::String,
            mutable: false,
            init: Some(Expr::String("./a.ts".into())),
        });
        m.init.push(Stmt::Let {
            id: 2,
            name: "mutated".into(),
            ty: perry_types::Type::String,
            mutable: false,
            init: Some(Expr::String("./b.ts".into())),
        });
        m.init.push(Stmt::Expr(Expr::LocalSet(
            2,
            Box::new(Expr::String("./c.ts".into())),
        )));
        let consts = collect_module_const_locals(&m);
        assert!(consts.contains_key(&1));
        assert!(!consts.contains_key(&2));
    }

    #[test]
    fn collect_includes_unreassigned_let_but_drops_reassigned() {
        // #1674: a `let` (mutable) that is never reassigned resolves like a
        // const; a reassigned one still falls back to Unresolved.
        let mut m = Module::new("t");
        m.init.push(Stmt::Let {
            id: 1,
            name: "stableLet".into(),
            ty: perry_types::Type::String,
            mutable: true,
            init: Some(Expr::String("./a.ts".into())),
        });
        m.init.push(Stmt::Let {
            id: 2,
            name: "reassignedLet".into(),
            ty: perry_types::Type::String,
            mutable: true,
            init: Some(Expr::String("./b.ts".into())),
        });
        m.init.push(Stmt::Expr(Expr::LocalSet(
            2,
            Box::new(Expr::String("./c.ts".into())),
        )));
        let consts = collect_module_const_locals(&m);
        assert!(matches!(consts.get(&1), Some(Expr::String(s)) if s == "./a.ts"));
        assert!(!consts.contains_key(&2));
    }

    #[test]
    fn resolve_unreassigned_let_ternary_union() {
        // The #1674 acceptance shape: `let p = cond ? './a.ts' : './b.ts'`.
        let mut m = Module::new("t");
        m.init.push(Stmt::Let {
            id: 5,
            name: "p".into(),
            ty: perry_types::Type::String,
            mutable: true,
            init: Some(Expr::Conditional {
                condition: Box::new(Expr::Bool(true)),
                then_expr: Box::new(Expr::String("./a.ts".into())),
                else_expr: Box::new(Expr::String("./b.ts".into())),
            }),
        });
        let consts = collect_module_const_locals(&m);
        let mut visiting = std::collections::HashSet::new();
        match resolve_import_path_with_consts(&Expr::LocalGet(5), &consts, &mut visiting) {
            Resolution::Set(mut v) => {
                v.sort();
                assert_eq!(v, vec!["./a.ts", "./b.ts"]);
            }
            Resolution::Unresolved(reason) => panic!("expected Set, got Unresolved: {reason}"),
        }
    }

    #[test]
    fn resolve_closure_local_const_specifier() {
        // #1725: `() => { const cfWorkers = "cloudflare:workers"; import(cfWorkers) }`
        // — the const lives inside a closure body (hono's getColorEnabledAsync
        // IIFE shape), not at module top level. It must be collected so the
        // specifier resolves instead of erroring "not a module-level const".
        let mut m = Module::new("t");
        let closure = Expr::Closure {
            func_id: 0,
            params: vec![],
            return_type: Type::Any,
            body: vec![Stmt::Let {
                id: 9,
                name: "cfWorkers".into(),
                ty: Type::String,
                mutable: false,
                init: Some(Expr::String("cloudflare:workers".into())),
            }],
            captures: vec![],
            mutable_captures: vec![],
            captures_this: false,
            enclosing_class: None,
            is_async: true,
            is_generator: false,
        };
        m.init.push(Stmt::Expr(closure));

        let consts = collect_module_const_locals(&m);
        assert!(
            consts.contains_key(&9),
            "const declared inside a closure body should be collected"
        );

        let mut visiting = std::collections::HashSet::new();
        match resolve_import_path_with_consts(&Expr::LocalGet(9), &consts, &mut visiting) {
            Resolution::Set(v) => assert_eq!(v, vec!["cloudflare:workers"]),
            other => panic!("expected resolved Set, got {:?}", other),
        }
    }

    #[test]
    fn collect_consts_invalidates_closure_mutation() {
        // Soundness: a binding reassigned inside a closure body must be dropped
        // from the const map (the mutation scan descends into closures, #1725).
        let mut m = Module::new("t");
        m.init.push(Stmt::Let {
            id: 5,
            name: "p".into(),
            ty: Type::String,
            mutable: false,
            init: Some(Expr::String("./a.ts".into())),
        });
        let closure = Expr::Closure {
            func_id: 0,
            params: vec![],
            return_type: Type::Any,
            body: vec![Stmt::Expr(Expr::LocalSet(
                5,
                Box::new(Expr::String("./b.ts".into())),
            ))],
            captures: vec![5],
            mutable_captures: vec![5],
            captures_this: false,
            enclosing_class: None,
            is_async: false,
            is_generator: false,
        };
        m.init.push(Stmt::Expr(closure));
        let consts = collect_module_const_locals(&m);
        assert!(
            !consts.contains_key(&5),
            "mutation inside closure must invalidate"
        );
    }

    #[test]
    fn flatten_local_named_exports() {
        let mut m = Module::new("foo");
        m.exports.push(Export::Named {
            local: "x".into(),
            exported: "x".into(),
        });
        m.exports.push(Export::Named {
            local: "_g".into(),
            exported: "greet".into(),
        });
        let map = std::collections::HashMap::from([("foo".to_string(), m.clone())]);
        let lookup = |s: &str| map.get(s);
        let flat = flatten_exports("foo", &lookup);
        assert_eq!(flat.len(), 2);
        assert_eq!(flat[0].name, "x");
        assert_eq!(flat[0].source_module, "foo");
        assert_eq!(flat[0].source_local, "x");
        assert_eq!(flat[1].name, "greet");
        assert_eq!(flat[1].source_local, "_g");
    }

    #[test]
    fn flatten_reexport_one_hop() {
        let mut barrel = Module::new("barrel");
        barrel.exports.push(Export::ReExport {
            source: "inner".into(),
            imported: "v".into(),
            exported: "v".into(),
        });
        let map = std::collections::HashMap::from([("barrel".to_string(), barrel.clone())]);
        let lookup = |s: &str| map.get(s);
        let flat = flatten_exports("barrel", &lookup);
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].name, "v");
        assert_eq!(flat[0].source_module, "inner");
        assert_eq!(flat[0].source_local, "v");
    }

    #[test]
    fn flatten_export_all_recursive() {
        let mut inner = Module::new("inner");
        inner.exports.push(Export::Named {
            local: "v".into(),
            exported: "v".into(),
        });
        let mut barrel = Module::new("barrel");
        barrel.exports.push(Export::ExportAll {
            source: "inner".into(),
        });
        let map = std::collections::HashMap::from([
            ("inner".to_string(), inner.clone()),
            ("barrel".to_string(), barrel.clone()),
        ]);
        let lookup = |s: &str| map.get(s);
        let flat = flatten_exports("barrel", &lookup);
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].name, "v");
        assert_eq!(flat[0].source_module, "inner");
        assert_eq!(flat[0].source_local, "v");
    }

    #[test]
    fn flatten_export_all_cycle_safe() {
        // a -> b -> a — must terminate.
        let mut a = Module::new("a");
        a.exports.push(Export::ExportAll { source: "b".into() });
        a.exports.push(Export::Named {
            local: "fromA".into(),
            exported: "fromA".into(),
        });
        let mut b = Module::new("b");
        b.exports.push(Export::ExportAll { source: "a".into() });
        b.exports.push(Export::Named {
            local: "fromB".into(),
            exported: "fromB".into(),
        });
        let map = std::collections::HashMap::from([
            ("a".to_string(), a.clone()),
            ("b".to_string(), b.clone()),
        ]);
        let lookup = |s: &str| map.get(s);
        let flat = flatten_exports("a", &lookup);
        // Both names appear; recursion terminates at the back-edge.
        let names: Vec<String> = flat.iter().map(|e| e.name.clone()).collect();
        assert!(names.contains(&"fromA".to_string()));
        assert!(names.contains(&"fromB".to_string()));
    }

    #[test]
    fn flatten_namespace_re_export() {
        let mut m = Module::new("m");
        m.exports.push(Export::NamespaceReExport {
            source: "sub".into(),
            name: "Sub".into(),
        });
        let map = std::collections::HashMap::from([("m".to_string(), m.clone())]);
        let lookup = |s: &str| map.get(s);
        let flat = flatten_exports("m", &lookup);
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].name, "Sub");
        assert_eq!(flat[0].nested_namespace_of, Some("sub".to_string()));
    }

    #[test]
    fn tla_skips_await_inside_closure() {
        let mut m = Module::new("t");
        // Build a closure body containing an Await — the module-level
        // detector must NOT descend into the closure.
        let closure = Expr::Closure {
            func_id: 0,
            params: vec![],
            return_type: Type::Any,
            body: vec![Stmt::Expr(Expr::Await(Box::new(Expr::Undefined)))],
            captures: vec![],
            mutable_captures: vec![],
            captures_this: false,
            enclosing_class: None,
            is_async: true,
            is_generator: false,
        };
        m.init.push(Stmt::Expr(closure));
        detect_top_level_await(&mut m);
        assert!(!m.has_top_level_await);
    }

    // #1674 sub-B: `("./plugins/" + name) + ".ts"` where `name` is a
    // non-resolvable local — the HIR shape of `` `./plugins/${name}.ts` ``.
    fn glob_chain(prefix: &str, suffix: &str, wild_id: u32) -> Expr {
        Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::String(prefix.into())),
                right: Box::new(Expr::LocalGet(wild_id)),
            }),
            right: Box::new(Expr::String(suffix.into())),
        }
    }

    #[test]
    fn glob_pattern_extracts_relative_prefix_and_suffix() {
        let consts = std::collections::HashMap::new();
        let arg = glob_chain("./plugins/", ".ts", 1);
        assert_eq!(
            dynamic_import_glob_pattern(&arg, &consts),
            Some(("./plugins/".to_string(), ".ts".to_string()))
        );
    }

    #[test]
    fn glob_pattern_rejects_non_relative_or_dirless_prefix() {
        let consts = std::collections::HashMap::new();
        // bare prefix with no directory component — too broad to glob.
        assert_eq!(
            dynamic_import_glob_pattern(&glob_chain("locale_", ".ts", 1), &consts),
            None
        );
        // absolute / package prefix — not a relative directory glob.
        assert_eq!(
            dynamic_import_glob_pattern(&glob_chain("@scope/", ".ts", 1), &consts),
            None
        );
    }

    #[test]
    fn glob_pattern_none_when_fully_resolvable() {
        // No wildcard part — the normal resolver handles this, not the glob.
        let consts = std::collections::HashMap::new();
        let arg = Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::String("./a".into())),
            right: Box::new(Expr::String(".ts".into())),
        };
        assert_eq!(dynamic_import_glob_pattern(&arg, &consts), None);
    }
}
