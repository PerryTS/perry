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

use crate::ir::{BinaryOp, Export, Expr, Function, Module, Param, Stmt};
use crate::walker::walk_expr_children;
use perry_types::Type;
use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};

/// Hard cap on the number of paths a single `import()` site can resolve
/// to. Over-cap produces a compile error per D2 (issue #100).
pub const DYNAMIC_IMPORT_PATH_CAP: usize = 64;

mod visitors;
pub use visitors::{
    for_each_dynamic_import, for_each_dynamic_import_mut, for_each_worker_new,
    for_each_worker_new_mut,
};

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
                // #6304: `export { run }` does NOT imply `run` is defined here.
                // When `run` is an *import* binding of this module —
                //     import { run } from "./chunk-XXXX.js";
                //     export { run };
                // — the value lives in `./chunk-XXXX.js`, not here. That two-
                // statement file is exactly the shape esbuild/bun emit for a
                // shared chunk under `--splitting`, so it is the common case,
                // not an edge case.
                //
                // Pre-fix this pushed `source_module = <this module>`, and the
                // driver's namespace-entry classifier then searched THIS
                // module's HIR for a function/class/global named `run`, found
                // nothing (it is an import, not a definition), and fell back to
                // a `ForeignVar` getter call on `perry_fn_<thismod>__run` —
                // which the #461 stub loop claims with an undefined-returning
                // stub. Net effect: the whole namespace of a re-export-only
                // chunk came out `undefined`, silently.
                //
                // Resolving the binding to its defining module instead lets the
                // classifier see the real function/class/global and emit the
                // proper closure/global reference.
                let origin =
                    resolve_binding_origin(module_name, local, lookup).unwrap_or_else(|| {
                        BindingOrigin {
                            source_module: module_name.to_string(),
                            source_local: local.clone(),
                            namespace_of: None,
                        }
                    });
                out.push(FlatExport {
                    name: exported.clone(),
                    source_module: origin.source_module,
                    source_local: origin.source_local,
                    nested_namespace_of: origin.namespace_of,
                });
            }
            Export::ReExport {
                source,
                imported,
                exported,
            } => {
                // The value lives in `source` — but `source` may itself
                // re-export it (a barrel chain, or a bundler emitting one
                // chunk that re-exports another). Follow the chain to the
                // ULTIMATE owner so the classifier sees a real definition
                // rather than another forwarding stub. When nothing can be
                // followed, `resolve_binding_origin` returns `None` and we
                // fall back to naming the directly-importing source — the
                // long-standing one-hop behaviour.
                let origin =
                    resolve_binding_origin(source, imported, lookup).unwrap_or_else(|| {
                        BindingOrigin {
                            source_module: source.clone(),
                            source_local: imported.clone(),
                            namespace_of: None,
                        }
                    });
                out.push(FlatExport {
                    name: exported.clone(),
                    source_module: origin.source_module,
                    source_local: origin.source_local,
                    nested_namespace_of: origin.namespace_of,
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

/// #6304: where an exported name's value actually lives, after following
/// import bindings and re-export hops through the module graph.
struct BindingOrigin {
    /// Module that owns the binding.
    source_module: String,
    /// The name the binding has *in* `source_module`.
    source_local: String,
    /// `Some(m)` when the binding is the module namespace of `m` rather than
    /// a plain value (`import * as X` / `export * as X`).
    namespace_of: Option<String>,
}

/// True when `module` actually *defines* `name` (as opposed to merely
/// importing or re-exporting it). A definition stops origin resolution.
fn defines_local_binding(module: &Module, name: &str) -> bool {
    module.functions.iter().any(|f| f.name == name)
        || module.classes.iter().any(|c| c.name == name)
        || module.globals.iter().any(|g| g.name == name)
        || module.enums.iter().any(|e| e.name == name)
}

/// The import binding (if any) that `name` refers to in `module`.
///
/// Native / builtin imports (`import { readFile } from "fs"`) are deliberately
/// excluded: their source is not a compiled module in the graph, so redirecting
/// an export to them would name a module that has no HIR and no
/// `perry_fn_*` symbols. Those keep the pre-existing local-lookup behaviour.
fn find_import_binding(module: &Module, name: &str) -> Option<(String, ImportBindingKind)> {
    for import in &module.imports {
        if import.type_only || import.is_native {
            continue;
        }
        for spec in &import.specifiers {
            match spec {
                crate::ir::ImportSpecifier::Named { imported, local } if local == name => {
                    return Some((
                        import.source.clone(),
                        ImportBindingKind::Value(imported.clone()),
                    ));
                }
                crate::ir::ImportSpecifier::Default { local } if local == name => {
                    return Some((
                        import.source.clone(),
                        ImportBindingKind::Value("default".to_string()),
                    ));
                }
                crate::ir::ImportSpecifier::Namespace { local } if local == name => {
                    return Some((import.source.clone(), ImportBindingKind::Namespace));
                }
                _ => {}
            }
        }
    }
    None
}

enum ImportBindingKind {
    /// A plain value binding; the payload is the name in the source module.
    Value(String),
    /// The whole module namespace of the import's source.
    Namespace,
}

/// #6304: resolve `(module_name, local)` to the module that actually defines
/// the binding, following import bindings and `ReExport` / `NamespaceReExport`
/// hops.
///
/// Returns `None` when nothing could be followed — either `module_name` already
/// defines the binding, or the chain leaves the compiled-module graph (a native
/// import, or a source we have no HIR for). Callers then keep their pre-existing
/// default, so this is strictly a refinement: it can only move an entry CLOSER
/// to a real definition, never invent one.
///
/// Cycle-safe: a `(module, name)` pair already visited terminates the walk, so a
/// self-referential barrel (`export * as Token from "./selfns"` inside
/// `selfns.ts`) cannot loop forever.
fn resolve_binding_origin<'a, F>(
    start_module: &str,
    start_local: &str,
    lookup: &F,
) -> Option<BindingOrigin>
where
    F: Fn(&str) -> Option<&'a Module>,
{
    let mut module_name = start_module.to_string();
    let mut local = start_local.to_string();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    // Only report an origin once we've actually moved somewhere new; otherwise
    // the caller's existing default already names the right module.
    let mut moved = false;

    loop {
        if !seen.insert((module_name.clone(), local.clone())) {
            break;
        }
        let Some(module) = lookup(&module_name) else {
            break;
        };
        // A real definition here — this is the owner.
        if defines_local_binding(module, &local) {
            break;
        }

        // `import { x } from "src"; export { x }` — hop to `src`.
        if let Some((source, kind)) = find_import_binding(module, &local) {
            if lookup(&source).is_none() {
                break;
            }
            match kind {
                ImportBindingKind::Value(imported) => {
                    module_name = source;
                    local = imported;
                    moved = true;
                    continue;
                }
                ImportBindingKind::Namespace => {
                    return Some(BindingOrigin {
                        source_module: source.clone(),
                        source_local: String::new(),
                        namespace_of: Some(source),
                    });
                }
            }
        }

        // `export { x } from "src"` / `export * as X from "src"` — hop through
        // the re-export. Lets a chain of barrels (or bundler chunks that
        // re-export one another) reach the ultimate owner.
        let mut hopped = false;
        for export in &module.exports {
            match export {
                Export::ReExport {
                    source,
                    imported,
                    exported,
                } if *exported == local => {
                    if lookup(source).is_none() {
                        break;
                    }
                    module_name = source.clone();
                    local = imported.clone();
                    moved = true;
                    hopped = true;
                    break;
                }
                Export::NamespaceReExport { source, name } if *name == local => {
                    if lookup(source).is_none() {
                        break;
                    }
                    return Some(BindingOrigin {
                        source_module: source.clone(),
                        source_local: String::new(),
                        namespace_of: Some(source.clone()),
                    });
                }
                _ => {}
            }
        }
        if hopped {
            continue;
        }
        break;
    }

    moved.then(|| BindingOrigin {
        source_module: module_name,
        source_local: local,
        namespace_of: None,
    })
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
pub fn collect_module_const_locals<'a>(
    module: &'a Module,
) -> std::collections::HashMap<u32, &'a Expr> {
    use std::collections::HashMap;
    let mut consts: HashMap<u32, &'a Expr> = HashMap::new();

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

/// #1674: collect function/closure parameters whose declared type is a finite
/// set of string literals. These locals can safely seed dynamic `import()`
/// candidate sets even though their runtime value is not constant.
pub fn collect_dynamic_import_param_literals(module: &Module) -> HashMap<u32, Vec<String>> {
    let mut out: HashMap<u32, Vec<String>> = HashMap::new();
    let type_aliases = dynamic_import_type_aliases(module);

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

    for func in funcs {
        collect_param_literal_sets(&func.params, &mut out, &type_aliases);
        for stmt in &func.body {
            collect_param_literal_sets_stmt(stmt, &mut out, &type_aliases);
        }
        for p in &func.params {
            if let Some(default) = &p.default {
                collect_param_literal_sets_expr(default, &mut out, &type_aliases);
            }
        }
    }
    for stmt in &module.init {
        collect_param_literal_sets_stmt(stmt, &mut out, &type_aliases);
    }
    for expr in init_exprs {
        collect_param_literal_sets_expr(expr, &mut out, &type_aliases);
    }

    out
}

/// #1674: collect locals whose full set of observed definitions is finite and
/// string-resolvable, even when the values come from later `LocalSet`
/// assignments instead of the declaration initializer.
///
/// This is intentionally a bounded candidate collector, not a full flow
/// analysis. If any observed definition for a local is not resolvable by the
/// existing dynamic-import resolver, the local is omitted so the import site
/// keeps the normal compile-time error.
pub fn collect_dynamic_import_local_candidate_literals<V: Borrow<Expr>>(
    module: &Module,
    consts: &HashMap<u32, V>,
    param_literals: &HashMap<u32, Vec<String>>,
) -> HashMap<u32, Vec<String>> {
    let mut defs: HashMap<u32, Vec<&Expr>> = HashMap::new();
    let mut invalid: HashSet<u32> = HashSet::new();

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
        collect_local_candidate_defs_stmt(stmt, &mut defs, &mut invalid);
    }
    for func in funcs {
        for stmt in &func.body {
            collect_local_candidate_defs_stmt(stmt, &mut defs, &mut invalid);
        }
        for param in &func.params {
            if let Some(default) = &param.default {
                collect_local_candidate_defs_expr(default, &mut defs, &mut invalid);
            }
        }
    }
    for expr in init_exprs {
        collect_local_candidate_defs_expr(expr, &mut defs, &mut invalid);
    }

    let mut out: HashMap<u32, Vec<String>> = HashMap::new();
    for (id, exprs) in defs {
        if invalid.contains(&id) {
            continue;
        }
        let mut candidates: Vec<String> = Vec::new();
        let mut ok = true;
        for expr in exprs {
            let mut visiting = HashSet::new();
            match resolve_import_path_with_consts_and_params(
                expr,
                consts,
                param_literals,
                &mut visiting,
            ) {
                Resolution::Set(paths) => {
                    for path in paths {
                        if !candidates.contains(&path) {
                            candidates.push(path);
                        }
                    }
                }
                Resolution::Unresolved(_) => {
                    ok = false;
                    break;
                }
            }
        }
        if ok && !candidates.is_empty() {
            out.insert(id, candidates);
        }
    }
    out
}

fn collect_local_candidate_defs_stmt<'a>(
    stmt: &'a Stmt,
    defs: &mut HashMap<u32, Vec<&'a Expr>>,
    invalid: &mut HashSet<u32>,
) {
    collect_local_candidate_defs_from_frames(
        &mut vec![LocalCandidateFrame::Stmt(stmt)],
        defs,
        invalid,
    );
}

fn collect_local_candidate_defs_expr<'a>(
    expr: &'a Expr,
    defs: &mut HashMap<u32, Vec<&'a Expr>>,
    invalid: &mut HashSet<u32>,
) {
    collect_local_candidate_defs_from_frames(
        &mut vec![LocalCandidateFrame::Expr(expr)],
        defs,
        invalid,
    );
}

enum LocalCandidateFrame<'a> {
    Stmt(&'a Stmt),
    Expr(&'a Expr),
}

fn collect_local_candidate_defs_from_frames<'a>(
    stack: &mut Vec<LocalCandidateFrame<'a>>,
    defs: &mut HashMap<u32, Vec<&'a Expr>>,
    invalid: &mut HashSet<u32>,
) {
    while let Some(frame) = stack.pop() {
        match frame {
            LocalCandidateFrame::Stmt(stmt) => match stmt {
                Stmt::Let {
                    id, init: Some(e), ..
                } => {
                    defs.entry(*id).or_default().push(e);
                    stack.push(LocalCandidateFrame::Expr(e));
                }
                Stmt::Let { init: None, .. } | Stmt::Return(None) => {}
                Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => {
                    stack.push(LocalCandidateFrame::Expr(e));
                }
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    if let Some(else_branch) = else_branch {
                        push_local_candidate_stmt_slice(stack, else_branch);
                    }
                    push_local_candidate_stmt_slice(stack, then_branch);
                    stack.push(LocalCandidateFrame::Expr(condition));
                }
                Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                    push_local_candidate_stmt_slice(stack, body);
                    stack.push(LocalCandidateFrame::Expr(condition));
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    push_local_candidate_stmt_slice(stack, body);
                    if let Some(update) = update {
                        stack.push(LocalCandidateFrame::Expr(update));
                    }
                    if let Some(condition) = condition {
                        stack.push(LocalCandidateFrame::Expr(condition));
                    }
                    if let Some(init) = init {
                        stack.push(LocalCandidateFrame::Stmt(init.as_ref()));
                    }
                }
                Stmt::Labeled { body, .. } => {
                    stack.push(LocalCandidateFrame::Stmt(body.as_ref()));
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    if let Some(finally) = finally {
                        push_local_candidate_stmt_slice(stack, finally);
                    }
                    if let Some(catch) = catch {
                        push_local_candidate_stmt_slice(stack, &catch.body);
                    }
                    push_local_candidate_stmt_slice(stack, body);
                }
                Stmt::Switch {
                    discriminant,
                    cases,
                } => {
                    for case in cases.iter().rev() {
                        push_local_candidate_stmt_slice(stack, &case.body);
                        if let Some(test) = &case.test {
                            stack.push(LocalCandidateFrame::Expr(test));
                        }
                    }
                    stack.push(LocalCandidateFrame::Expr(discriminant));
                }
                Stmt::Break
                | Stmt::Continue
                | Stmt::LabeledBreak(_)
                | Stmt::LabeledContinue(_)
                | Stmt::PreallocateBoxes(_)
                | Stmt::PreallocateTdzBoxes(_) => {}
            },
            LocalCandidateFrame::Expr(expr) => {
                match expr {
                    Expr::LocalSet(id, value) => {
                        defs.entry(*id).or_default().push(value);
                    }
                    Expr::Update { id, .. } => {
                        invalid.insert(*id);
                    }
                    Expr::Closure { body, .. } => {
                        push_local_candidate_stmt_slice(stack, body);
                    }
                    _ => {}
                }
                let mut children = Vec::new();
                walk_expr_children(expr, &mut |child| {
                    children.push(child);
                });
                for child in children.into_iter().rev() {
                    stack.push(LocalCandidateFrame::Expr(child));
                }
            }
        }
    }
}

fn push_local_candidate_stmt_slice<'a>(
    stack: &mut Vec<LocalCandidateFrame<'a>>,
    stmts: &'a [Stmt],
) {
    for stmt in stmts.iter().rev() {
        stack.push(LocalCandidateFrame::Stmt(stmt));
    }
}

fn collect_param_literal_sets(
    params: &[Param],
    out: &mut std::collections::HashMap<u32, Vec<String>>,
    type_aliases: &HashMap<String, &Type>,
) {
    for param in params {
        if let Some(paths) = string_literal_type_set(&param.ty, type_aliases) {
            out.insert(param.id, paths);
        }
    }
}

fn dynamic_import_type_aliases(module: &Module) -> HashMap<String, &Type> {
    let mut aliases = HashMap::new();
    for alias in &module.type_aliases {
        if alias.type_params.is_empty() {
            aliases.entry(alias.name.clone()).or_insert(&alias.ty);
        }
    }
    aliases
}

fn string_literal_type_set(
    ty: &Type,
    type_aliases: &HashMap<String, &Type>,
) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut visiting = HashSet::new();
    collect_string_literal_type_set(ty, type_aliases, &mut visiting, &mut out)?;
    if out.is_empty() {
        return None;
    }
    Some(out)
}

fn collect_string_literal_type_set(
    ty: &Type,
    type_aliases: &HashMap<String, &Type>,
    visiting: &mut HashSet<String>,
    out: &mut Vec<String>,
) -> Option<()> {
    match ty {
        Type::StringLiteral(s) => {
            if !out.contains(s) {
                out.push(s.clone());
            }
            Some(())
        }
        Type::Union(types) => {
            for ty in types {
                collect_string_literal_type_set(ty, type_aliases, visiting, out)?;
            }
            Some(())
        }
        Type::Named(name) => {
            let aliased = type_aliases.get(name)?;
            if !visiting.insert(name.clone()) {
                return None;
            }
            let resolved = collect_string_literal_type_set(aliased, type_aliases, visiting, out);
            visiting.remove(name);
            resolved
        }
        _ => None,
    }
}

fn collect_param_literal_sets_stmt(
    stmt: &Stmt,
    out: &mut std::collections::HashMap<u32, Vec<String>>,
    type_aliases: &HashMap<String, &Type>,
) {
    collect_param_literal_sets_from_frames(&mut vec![ParamFrame::Stmt(stmt)], out, type_aliases);
}

fn collect_param_literal_sets_expr(
    expr: &Expr,
    out: &mut std::collections::HashMap<u32, Vec<String>>,
    type_aliases: &HashMap<String, &Type>,
) {
    collect_param_literal_sets_from_frames(&mut vec![ParamFrame::Expr(expr)], out, type_aliases);
}

enum ParamFrame<'a> {
    Stmt(&'a Stmt),
    Expr(&'a Expr),
}

fn collect_param_literal_sets_from_frames(
    stack: &mut Vec<ParamFrame<'_>>,
    out: &mut std::collections::HashMap<u32, Vec<String>>,
    type_aliases: &HashMap<String, &Type>,
) {
    while let Some(frame) = stack.pop() {
        match frame {
            ParamFrame::Stmt(stmt) => match stmt {
                Stmt::Let { init: Some(e), .. }
                | Stmt::Expr(e)
                | Stmt::Throw(e)
                | Stmt::Return(Some(e)) => {
                    stack.push(ParamFrame::Expr(e));
                }
                Stmt::Let { init: None, .. } | Stmt::Return(None) => {}
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    if let Some(else_branch) = else_branch {
                        push_param_stmt_slice(stack, else_branch);
                    }
                    push_param_stmt_slice(stack, then_branch);
                    stack.push(ParamFrame::Expr(condition));
                }
                Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                    push_param_stmt_slice(stack, body);
                    stack.push(ParamFrame::Expr(condition));
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    push_param_stmt_slice(stack, body);
                    if let Some(update) = update {
                        stack.push(ParamFrame::Expr(update));
                    }
                    if let Some(condition) = condition {
                        stack.push(ParamFrame::Expr(condition));
                    }
                    if let Some(init) = init {
                        stack.push(ParamFrame::Stmt(init.as_ref()));
                    }
                }
                Stmt::Labeled { body, .. } => {
                    stack.push(ParamFrame::Stmt(body.as_ref()));
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    if let Some(finally) = finally {
                        push_param_stmt_slice(stack, finally);
                    }
                    if let Some(catch) = catch {
                        push_param_stmt_slice(stack, &catch.body);
                    }
                    push_param_stmt_slice(stack, body);
                }
                Stmt::Switch {
                    discriminant,
                    cases,
                } => {
                    for case in cases.iter().rev() {
                        push_param_stmt_slice(stack, &case.body);
                        if let Some(test) = &case.test {
                            stack.push(ParamFrame::Expr(test));
                        }
                    }
                    stack.push(ParamFrame::Expr(discriminant));
                }
                Stmt::Break
                | Stmt::Continue
                | Stmt::LabeledBreak(_)
                | Stmt::LabeledContinue(_)
                | Stmt::PreallocateBoxes(_)
                | Stmt::PreallocateTdzBoxes(_) => {}
            },
            ParamFrame::Expr(expr) => {
                if let Expr::Closure { params, body, .. } = expr {
                    collect_param_literal_sets(params, out, type_aliases);
                    push_param_stmt_slice(stack, body);
                    for param in params {
                        if let Some(default) = &param.default {
                            stack.push(ParamFrame::Expr(default));
                        }
                    }
                }
                let mut children = Vec::new();
                walk_expr_children(expr, &mut |child| {
                    children.push(child);
                });
                for child in children.into_iter().rev() {
                    stack.push(ParamFrame::Expr(child));
                }
            }
        }
    }
}

fn push_param_stmt_slice<'a>(stack: &mut Vec<ParamFrame<'a>>, stmts: &'a [Stmt]) {
    for stmt in stmts.iter().rev() {
        stack.push(ParamFrame::Stmt(stmt));
    }
}

/// Collect `const x = <init>` bindings reachable from `stmt` into `out`,
/// recursing through nested blocks (#1725). Mirrors `scan_mutations_stmt`'s
/// traversal and additionally descends into closure bodies via
/// `collect_const_locals_expr`.
fn collect_const_locals_stmt<'a>(
    stmt: &'a Stmt,
    out: &mut std::collections::HashMap<u32, &'a Expr>,
) {
    collect_const_locals_from_frames(&mut vec![ConstFrame::Stmt(stmt)], out);
}

/// Descend into an expression collecting const locals declared inside closure
/// bodies (`walk_expr_children` deliberately skips closure bodies, so handle
/// them explicitly). #1725.
fn collect_const_locals_expr<'a>(
    expr: &'a Expr,
    out: &mut std::collections::HashMap<u32, &'a Expr>,
) {
    collect_const_locals_from_frames(&mut vec![ConstFrame::Expr(expr)], out);
}

enum ConstFrame<'a> {
    Stmt(&'a Stmt),
    Expr(&'a Expr),
}

fn collect_const_locals_from_frames<'a>(
    stack: &mut Vec<ConstFrame<'a>>,
    out: &mut std::collections::HashMap<u32, &'a Expr>,
) {
    while let Some(frame) = stack.pop() {
        match frame {
            ConstFrame::Stmt(stmt) => {
                match stmt {
                    Stmt::Let {
                        id, init: Some(e), ..
                    } => {
                        // #1674: collect both `const` and never-reassigned
                        // `let`/`var` bindings. Keep a borrowed initializer so
                        // large schema-shaped expressions are not cloned during
                        // dynamic-import analysis.
                        out.insert(*id, e);
                        stack.push(ConstFrame::Expr(e));
                    }
                    Stmt::Let { init: None, .. } => {}
                    Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => {
                        stack.push(ConstFrame::Expr(e));
                    }
                    Stmt::Return(None) => {}
                    Stmt::If {
                        condition,
                        then_branch,
                        else_branch,
                    } => {
                        if let Some(eb) = else_branch {
                            push_const_stmt_slice(stack, eb);
                        }
                        push_const_stmt_slice(stack, then_branch);
                        stack.push(ConstFrame::Expr(condition));
                    }
                    Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                        push_const_stmt_slice(stack, body);
                        stack.push(ConstFrame::Expr(condition));
                    }
                    Stmt::For {
                        init,
                        condition,
                        update,
                        body,
                    } => {
                        push_const_stmt_slice(stack, body);
                        if let Some(u) = update {
                            stack.push(ConstFrame::Expr(u));
                        }
                        if let Some(c) = condition {
                            stack.push(ConstFrame::Expr(c));
                        }
                        if let Some(i) = init {
                            stack.push(ConstFrame::Stmt(i.as_ref()));
                        }
                    }
                    Stmt::Labeled { body, .. } => {
                        stack.push(ConstFrame::Stmt(body.as_ref()));
                    }
                    Stmt::Try {
                        body,
                        catch,
                        finally,
                    } => {
                        if let Some(fb) = finally {
                            push_const_stmt_slice(stack, fb);
                        }
                        if let Some(c) = catch {
                            push_const_stmt_slice(stack, &c.body);
                        }
                        push_const_stmt_slice(stack, body);
                    }
                    Stmt::Switch {
                        discriminant,
                        cases,
                    } => {
                        for case in cases.iter().rev() {
                            push_const_stmt_slice(stack, &case.body);
                            if let Some(t) = &case.test {
                                stack.push(ConstFrame::Expr(t));
                            }
                        }
                        stack.push(ConstFrame::Expr(discriminant));
                    }
                    Stmt::Break
                    | Stmt::Continue
                    | Stmt::LabeledBreak(_)
                    | Stmt::LabeledContinue(_)
                    | Stmt::PreallocateBoxes(_)
                    | Stmt::PreallocateTdzBoxes(_) => {}
                }
            }
            ConstFrame::Expr(expr) => {
                if let Expr::Closure { body, .. } = expr {
                    push_const_stmt_slice(stack, body);
                }
                let mut children = Vec::new();
                walk_expr_children(expr, &mut |child| {
                    children.push(child);
                });
                for child in children.into_iter().rev() {
                    stack.push(ConstFrame::Expr(child));
                }
            }
        }
    }
}

fn push_const_stmt_slice<'a>(stack: &mut Vec<ConstFrame<'a>>, stmts: &'a [Stmt]) {
    for stmt in stmts.iter().rev() {
        stack.push(ConstFrame::Stmt(stmt));
    }
}

fn scan_mutations_stmt(stmt: &Stmt, out: &mut std::collections::HashSet<u32>) {
    scan_mutations_from_frames(&mut vec![MutationFrame::Stmt(stmt as *const Stmt)], out);
}

fn scan_mutations_expr(expr: &Expr, out: &mut std::collections::HashSet<u32>) {
    scan_mutations_from_frames(&mut vec![MutationFrame::Expr(expr as *const Expr)], out);
}

enum MutationFrame {
    Stmt(*const Stmt),
    Expr(*const Expr),
}

fn scan_mutations_from_frames(
    stack: &mut Vec<MutationFrame>,
    out: &mut std::collections::HashSet<u32>,
) {
    while let Some(frame) = stack.pop() {
        match frame {
            MutationFrame::Stmt(stmt) => {
                let stmt = unsafe { &*stmt };
                match stmt {
                    Stmt::Let { init: Some(e), .. }
                    | Stmt::Expr(e)
                    | Stmt::Throw(e)
                    | Stmt::Return(Some(e)) => {
                        stack.push(MutationFrame::Expr(e as *const Expr));
                    }
                    Stmt::Let { init: None, .. } | Stmt::Return(None) => {}
                    Stmt::If {
                        condition,
                        then_branch,
                        else_branch,
                    } => {
                        if let Some(eb) = else_branch {
                            push_mutation_stmt_slice(stack, eb);
                        }
                        push_mutation_stmt_slice(stack, then_branch);
                        stack.push(MutationFrame::Expr(condition as *const Expr));
                    }
                    Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                        push_mutation_stmt_slice(stack, body);
                        stack.push(MutationFrame::Expr(condition as *const Expr));
                    }
                    Stmt::For {
                        init,
                        condition,
                        update,
                        body,
                    } => {
                        push_mutation_stmt_slice(stack, body);
                        if let Some(u) = update {
                            stack.push(MutationFrame::Expr(u as *const Expr));
                        }
                        if let Some(c) = condition {
                            stack.push(MutationFrame::Expr(c as *const Expr));
                        }
                        if let Some(i) = init {
                            stack.push(MutationFrame::Stmt(i.as_ref() as *const Stmt));
                        }
                    }
                    Stmt::Labeled { body, .. } => {
                        stack.push(MutationFrame::Stmt(body.as_ref() as *const Stmt));
                    }
                    Stmt::Try {
                        body,
                        catch,
                        finally,
                    } => {
                        if let Some(fb) = finally {
                            push_mutation_stmt_slice(stack, fb);
                        }
                        if let Some(c) = catch {
                            push_mutation_stmt_slice(stack, &c.body);
                        }
                        push_mutation_stmt_slice(stack, body);
                    }
                    Stmt::Switch {
                        discriminant,
                        cases,
                    } => {
                        for case in cases.iter().rev() {
                            push_mutation_stmt_slice(stack, &case.body);
                            if let Some(t) = &case.test {
                                stack.push(MutationFrame::Expr(t as *const Expr));
                            }
                        }
                        stack.push(MutationFrame::Expr(discriminant as *const Expr));
                    }
                    Stmt::Break
                    | Stmt::Continue
                    | Stmt::LabeledBreak(_)
                    | Stmt::LabeledContinue(_)
                    | Stmt::PreallocateBoxes(_)
                    | Stmt::PreallocateTdzBoxes(_) => {}
                }
            }
            MutationFrame::Expr(expr) => {
                let expr = unsafe { &*expr };
                match expr {
                    Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
                        out.insert(*id);
                    }
                    _ => {}
                }
                // `walk_expr_children` deliberately skips closure bodies;
                // descend manually so a reassignment inside a nested closure
                // still invalidates the entry (#1725).
                if let Expr::Closure { body, .. } = expr {
                    push_mutation_stmt_slice(stack, body);
                }
                let mut children = Vec::new();
                walk_expr_children(expr, &mut |child| {
                    children.push(child as *const Expr);
                });
                for child in children.into_iter().rev() {
                    stack.push(MutationFrame::Expr(child));
                }
            }
        }
    }
}

fn push_mutation_stmt_slice(stack: &mut Vec<MutationFrame>, stmts: &[Stmt]) {
    for stmt in stmts.iter().rev() {
        stack.push(MutationFrame::Stmt(stmt as *const Stmt));
    }
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
    let empty: std::collections::HashMap<u32, &Expr> = std::collections::HashMap::new();
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
pub fn dynamic_import_glob_pattern<V: Borrow<Expr>>(
    arg: &Expr,
    consts: &std::collections::HashMap<u32, V>,
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

pub fn resolve_import_path_with_consts<V: Borrow<Expr>>(
    arg: &Expr,
    consts: &std::collections::HashMap<u32, V>,
    visiting: &mut std::collections::HashSet<u32>,
) -> Resolution {
    let params: std::collections::HashMap<u32, Vec<String>> = std::collections::HashMap::new();
    resolve_import_path_with_consts_and_params(arg, consts, &params, visiting)
}

pub fn resolve_import_path_with_consts_and_params<V: Borrow<Expr>>(
    arg: &Expr,
    consts: &std::collections::HashMap<u32, V>,
    param_literals: &std::collections::HashMap<u32, Vec<String>>,
    visiting: &mut std::collections::HashSet<u32>,
) -> Resolution {
    let local_literals: HashMap<u32, Vec<String>> = HashMap::new();
    resolve_import_path_with_context(arg, consts, param_literals, &local_literals, visiting)
}

pub fn resolve_import_path_with_context<V: Borrow<Expr>>(
    arg: &Expr,
    consts: &std::collections::HashMap<u32, V>,
    param_literals: &std::collections::HashMap<u32, Vec<String>>,
    local_literals: &std::collections::HashMap<u32, Vec<String>>,
    visiting: &mut std::collections::HashSet<u32>,
) -> Resolution {
    match arg {
        Expr::String(s) => Resolution::Set(vec![s.clone()]),
        Expr::Call { callee, args, .. } => match static_string_replace_target(callee, args) {
            Some(string) => resolve_string_replace_parts(
                string,
                &args[0],
                &args[1],
                consts,
                param_literals,
                local_literals,
                visiting,
            ),
            None if is_static_path_join_call(callee) => {
                resolve_static_path_args(args, consts, param_literals, local_literals, visiting)
            }
            None => Resolution::Unresolved(NOT_STATICALLY_RESOLVABLE.to_string()),
        },
        Expr::PathJoin(left, right) | Expr::PathResolveJoin(left, right) => {
            let left = resolve_import_path_with_context(
                left,
                consts,
                param_literals,
                local_literals,
                visiting,
            );
            let right = resolve_import_path_with_context(
                right,
                consts,
                param_literals,
                local_literals,
                visiting,
            );
            match (left, right) {
                (Resolution::Set(lefts), Resolution::Set(rights)) => {
                    let mut out = Vec::new();
                    for left in &lefts {
                        for right in &rights {
                            let joined = static_path_join(left, right);
                            if !out.contains(&joined) {
                                out.push(joined);
                            }
                        }
                    }
                    Resolution::Set(out)
                }
                (Resolution::Unresolved(reason), _) | (_, Resolution::Unresolved(reason)) => {
                    Resolution::Unresolved(reason)
                }
            }
        }
        Expr::StringReplace {
            string,
            pattern,
            replacement,
        } => resolve_string_replace_parts(
            string,
            pattern,
            replacement,
            consts,
            param_literals,
            local_literals,
            visiting,
        ),
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            let a = resolve_import_path_with_context(
                then_expr,
                consts,
                param_literals,
                local_literals,
                visiting,
            );
            let b = resolve_import_path_with_context(
                else_expr,
                consts,
                param_literals,
                local_literals,
                visiting,
            );
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
                match resolve_import_path_with_context(
                    p,
                    consts,
                    param_literals,
                    local_literals,
                    visiting,
                ) {
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
                resolve_import_path_with_context(
                    init.borrow(),
                    consts,
                    param_literals,
                    local_literals,
                    visiting,
                )
            } else if let Some(paths) = param_literals.get(id) {
                Resolution::Set(paths.clone())
            } else if let Some(paths) = local_literals.get(id) {
                Resolution::Set(paths.clone())
            } else {
                Resolution::Unresolved(
                    "path argument references a binding that is not statically \
                     resolvable to a literal (supported: string literals, ternaries, \
                     template literals over resolvable locals, and `const`/never-\
                     reassigned `let` bindings initialized to a resolvable value, \
                     parameters annotated with finite string-literal unions, and \
                     locals whose observed assignments form a finite string-literal \
                     candidate set; broad or mixed parameter/local values fall back here)"
                        .to_string(),
                )
            };
            visiting.remove(id);
            resolved
        }
        // #5207 registry-object pattern: `const R = { a: "./chunk-a.js", … };
        // import(R[key])` / `import(R.a)`. Bundlers (and hand-written lazy-load
        // tables) map a route/feature key to a statically-knowable chunk path
        // through a const object literal — the exact "enumerate with a registry
        // object" shape the over-cap note already advertises. Either member form
        // resolves to the union of the registry's value specifiers (the whole
        // chunk set is what we want to ingest, and the runtime dispatch still
        // picks the right one by path string).
        //
        // Guard rail: this only fires when *every* value resolves to a relative
        // module specifier (`./…` / `../…`). That keeps it a strict
        // deferrals-into-compiles change — a plain data object indexed for a
        // non-module reason (`const cfg = { name: "app", port: "3000" };
        // import(cfg[k])`) has non-relative values, so it stays deferred exactly
        // as before instead of trying to compile `"app"`/`"3000"` as modules.
        Expr::PropertyGet { object, .. } | Expr::IndexGet { object, .. } => {
            // Record every const-local id on the registry's indirection chain in
            // the *outer* `visiting` set so a value that member-accesses back
            // into this (or an enclosing) registry is caught as a cycle rather
            // than recursing forever — `const R5 = { a: R6[x] }; const R6 = { b:
            // R5[y] }` is valid TS and would otherwise overflow the stack. The
            // chain ids are removed again before returning so sibling
            // resolutions still see those bindings.
            let mut chain_ids: Vec<u32> = Vec::new();
            let resolved = match object_registry_values(object, consts, visiting, &mut chain_ids) {
                Some(values) => resolve_registry_value_union(
                    &values,
                    consts,
                    param_literals,
                    local_literals,
                    visiting,
                ),
                None => Resolution::Unresolved(NOT_STATICALLY_RESOLVABLE.to_string()),
            };
            for id in &chain_ids {
                visiting.remove(id);
            }
            resolved
        }
        _ => Resolution::Unresolved(NOT_STATICALLY_RESOLVABLE.to_string()),
    }
}

/// True for a relative module specifier (`./x`, `../x`). Registry-object
/// dynamic-import resolution (#5207) only over-approximates to values that look
/// like relative chunk paths, so a non-module data object stays deferred.
fn is_relative_specifier(s: &str) -> bool {
    s.starts_with("./") || s.starts_with("../") || s == "." || s == ".."
}

/// #5207: collect the candidate value expressions of a const object-literal
/// "registry", following `const`-local indirection (`const R = { … };
/// import(R[k])`). Handles both object-literal HIR shapes: open-shape literals
/// kept as [`Expr::Object`], and closed-shape literals lowered to
/// `new __AnonShape_…(value0, value1, …)` (the constructor args are the field
/// values in declaration order — see `lower::expr_object`). Returns `None` for
/// anything that isn't statically one of those, so member access on an opaque
/// binding falls back to deferral.
///
/// Every traversed `LocalGet` id is inserted into `visiting` (and pushed onto
/// `chain_ids` so the caller can undo it) — a binding already in `visiting`
/// breaks the chain with `None`. Sharing the resolver's `visiting` set is what
/// makes cycle detection span the recursion back through
/// [`resolve_registry_value_union`], not just a single indirection chain.
fn object_registry_values<'a, V: Borrow<Expr>>(
    object: &'a Expr,
    consts: &'a std::collections::HashMap<u32, V>,
    visiting: &mut std::collections::HashSet<u32>,
    chain_ids: &mut Vec<u32>,
) -> Option<Vec<&'a Expr>> {
    match object {
        Expr::Object(entries) => Some(entries.iter().map(|(_, v)| v).collect()),
        Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => Some(args.iter().collect()),
        Expr::LocalGet(id) => {
            if !visiting.insert(*id) {
                return None;
            }
            chain_ids.push(*id);
            object_registry_values(consts.get(id)?.borrow(), consts, visiting, chain_ids)
        }
        _ => None,
    }
}

/// #5207: resolve a registry's value expressions to the union of their module
/// specifiers, but only when *every* value resolves to a relative specifier.
/// Any non-relative or unresolvable value collapses the whole site to
/// `Unresolved` so it keeps deferring (no false compile of a non-module string).
fn resolve_registry_value_union<V: Borrow<Expr>>(
    values: &[&Expr],
    consts: &std::collections::HashMap<u32, V>,
    param_literals: &std::collections::HashMap<u32, Vec<String>>,
    local_literals: &std::collections::HashMap<u32, Vec<String>>,
    visiting: &mut std::collections::HashSet<u32>,
) -> Resolution {
    let mut out: Vec<String> = Vec::new();
    for value in values {
        match resolve_import_path_with_context(
            value,
            consts,
            param_literals,
            local_literals,
            visiting,
        ) {
            Resolution::Set(set) => {
                for s in set {
                    if !is_relative_specifier(&s) {
                        return Resolution::Unresolved(NOT_STATICALLY_RESOLVABLE.to_string());
                    }
                    if !out.contains(&s) {
                        out.push(s);
                    }
                }
            }
            Resolution::Unresolved(reason) => return Resolution::Unresolved(reason),
        }
    }
    if out.is_empty() {
        Resolution::Unresolved(NOT_STATICALLY_RESOLVABLE.to_string())
    } else {
        Resolution::Set(out)
    }
}

/// Shared "couldn't statically resolve this dynamic import() specifier"
/// message used by the resolver's fall-through arms.
const NOT_STATICALLY_RESOLVABLE: &str =
    "path argument is not statically resolvable (supported: string literals, \
     ternaries of resolvable arms, template literals with const-local \
     interpolations, const object-literal registries indexed by a known or \
     computed key, and references to module-level const string locals)";

fn static_string_replace_target<'a>(callee: &'a Expr, args: &[Expr]) -> Option<&'a Expr> {
    if args.len() < 2 {
        return None;
    }
    match callee {
        Expr::PropertyGet {
            object, property, ..
        } if property == "replace" => Some(object),
        _ => None,
    }
}

fn resolve_string_replace_parts<V: Borrow<Expr>>(
    string: &Expr,
    pattern: &Expr,
    replacement: &Expr,
    consts: &std::collections::HashMap<u32, V>,
    param_literals: &std::collections::HashMap<u32, Vec<String>>,
    local_literals: &std::collections::HashMap<u32, Vec<String>>,
    visiting: &mut std::collections::HashSet<u32>,
) -> Resolution {
    let string =
        resolve_import_path_with_context(string, consts, param_literals, local_literals, visiting);
    let pattern =
        resolve_import_path_with_context(pattern, consts, param_literals, local_literals, visiting);
    let replacement = resolve_import_path_with_context(
        replacement,
        consts,
        param_literals,
        local_literals,
        visiting,
    );
    match (string, pattern, replacement) {
        (Resolution::Set(strings), Resolution::Set(patterns), Resolution::Set(replacements)) => {
            let mut out = Vec::new();
            for string in &strings {
                for pattern in &patterns {
                    for replacement in &replacements {
                        let replaced = string.replacen(pattern, replacement, 1);
                        if !out.contains(&replaced) {
                            out.push(replaced);
                        }
                    }
                }
            }
            Resolution::Set(out)
        }
        (Resolution::Unresolved(reason), _, _)
        | (_, Resolution::Unresolved(reason), _)
        | (_, _, Resolution::Unresolved(reason)) => Resolution::Unresolved(reason),
    }
}

fn is_static_path_join_call(callee: &Expr) -> bool {
    let Expr::PropertyGet {
        object, property, ..
    } = callee
    else {
        return false;
    };
    property == "join" && is_static_path_module_expr(object)
}

fn is_static_path_module_expr(expr: &Expr) -> bool {
    match expr {
        Expr::NativeModuleRef(module) => module == "path" || module == "node:path",
        Expr::PropertyGet {
            object, property, ..
        } if property == "default" => is_static_path_module_expr(object),
        _ => false,
    }
}

fn resolve_static_path_args<V: Borrow<Expr>>(
    args: &[Expr],
    consts: &std::collections::HashMap<u32, V>,
    param_literals: &std::collections::HashMap<u32, Vec<String>>,
    local_literals: &std::collections::HashMap<u32, Vec<String>>,
    visiting: &mut std::collections::HashSet<u32>,
) -> Resolution {
    if args.is_empty() {
        return Resolution::Set(vec![".".to_string()]);
    }
    let mut sets = Vec::with_capacity(args.len());
    for arg in args {
        match resolve_import_path_with_context(
            arg,
            consts,
            param_literals,
            local_literals,
            visiting,
        ) {
            Resolution::Set(paths) => sets.push(paths),
            Resolution::Unresolved(reason) => return Resolution::Unresolved(reason),
        }
    }
    let mut acc = vec![String::new()];
    for set in sets {
        let mut next = Vec::new();
        for left in &acc {
            for right in &set {
                let joined = static_path_join(left, right);
                if !next.contains(&joined) {
                    next.push(joined);
                }
            }
        }
        acc = next;
        if acc.len() > DYNAMIC_IMPORT_PATH_CAP {
            return Resolution::Set(acc);
        }
    }
    Resolution::Set(acc)
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

fn static_path_join(left: &str, right: &str) -> String {
    let left = left.replace('\\', "/");
    let right = right.replace('\\', "/");
    if is_static_path_absolute(&right) {
        return normalize_static_path(&right);
    }
    if left.is_empty() {
        return normalize_static_path(&right);
    }
    if right.is_empty() {
        return normalize_static_path(&left);
    }
    normalize_static_path(&format!(
        "{}/{}",
        left.trim_end_matches('/'),
        right.trim_start_matches('/')
    ))
}

fn is_static_path_absolute(path: &str) -> bool {
    path.starts_with('/')
        || path.starts_with("//")
        || path.as_bytes().get(1).is_some_and(|b| *b == b':')
}

fn normalize_static_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    let (prefix, rest) = split_static_path_prefix(&path);
    let mut parts = Vec::new();
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." if !parts.is_empty() && parts.last() != Some(&"..") => {
                parts.pop();
            }
            ".." if prefix.is_empty() => parts.push(part),
            ".." => {}
            _ => parts.push(part),
        }
    }
    let body = parts.join("/");
    if prefix.is_empty() {
        if body.is_empty() {
            ".".to_string()
        } else {
            body
        }
    } else if body.is_empty() {
        prefix.to_string()
    } else if prefix.ends_with('/') {
        format!("{prefix}{body}")
    } else {
        format!("{prefix}/{body}")
    }
}

fn split_static_path_prefix(path: &str) -> (&str, &str) {
    if path.starts_with("//") {
        let trimmed = path.trim_start_matches('/');
        return ("//", trimmed);
    }
    if path.starts_with('/') {
        return ("/", &path[1..]);
    }
    if path.as_bytes().get(1).is_some_and(|b| *b == b':') {
        return (&path[..2], &path[2..]);
    }
    ("", path)
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
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => false,
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
mod tests;
