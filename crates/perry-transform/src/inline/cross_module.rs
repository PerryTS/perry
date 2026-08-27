use perry_hir::types::{FuncId, LocalId};
use perry_hir::walker::{walk_expr_children, walk_expr_children_mut};
use perry_hir::{Class, Expr, Function, ImportSpecifier, Module, Stmt};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

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
            // Conservative: a body carrying a box-release hint is an async
            // step machine; never harvest it for cross-module inlining.
            Stmt::ReleaseBoxes(_) => false,
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

const MAX_CROSS_MODULE_FUNCTION_GRAPH: usize = 8;
const MAX_CROSS_MODULE_FUNCTION_STMTS: usize = 64;
const MAX_LOCALIZED_FUNCTION_ROOTS: usize = 32;

/// Harvest small exported functions together with the bounded graph of small
/// same-module helpers they call. Keeping the helper graph intact is
/// important: a copied source `FuncId` has no meaning in an importing module.
/// The destination-side localizer below assigns the entire graph fresh IDs,
/// so an unexpanded call always has a valid local fallback.
pub fn gather_cross_module_functions(module: &Module) -> HashMap<String, FunctionCandidate> {
    let functions: HashMap<FuncId, &Function> =
        module.functions.iter().map(|f| (f.id, f)).collect();
    let mut import_by_local: HashMap<String, RequiredExternImport> = HashMap::new();
    for import in &module.imports {
        let Some(path) = &import.resolved_path else {
            continue;
        };
        for specifier in &import.specifiers {
            if let ImportSpecifier::Named { imported, local } = specifier {
                import_by_local.insert(
                    local.clone(),
                    RequiredExternImport {
                        local: local.clone(),
                        imported: imported.clone(),
                        resolved_path: path.clone(),
                    },
                );
            }
        }
    }

    // A destination that imports only a function does not necessarily import
    // the source module's classes. Anon-shape classes are the one safe
    // exception: their names are content-addressed and the existing anon-class
    // propagation pass installs their definitions in the destination.
    let source_class_names: HashSet<String> = module
        .classes
        .iter()
        .flat_map(|class| std::iter::once(class.name.clone()).chain(class.aliases.iter().cloned()))
        .filter(|name| !name.starts_with("__AnonShape_"))
        .collect();

    let mut out = HashMap::new();
    for (exported_name, root_id) in &module.exported_functions {
        let mut visiting = HashSet::new();
        let mut visited = HashSet::new();
        let mut graph_ids = Vec::new();
        if !collect_function_graph(
            *root_id,
            &functions,
            &mut visiting,
            &mut visited,
            &mut graph_ids,
        ) {
            continue;
        }
        if graph_ids.len() > MAX_CROSS_MODULE_FUNCTION_GRAPH {
            continue;
        }

        let allowed_ids: HashSet<FuncId> = graph_ids.iter().copied().collect();
        let mut extern_names = Vec::new();
        let mut graph = Vec::with_capacity(graph_ids.len());
        let mut stmt_count = 0usize;
        let mut safe = true;
        for id in graph_ids {
            let Some(function) = functions.get(&id).copied() else {
                safe = false;
                break;
            };
            stmt_count = stmt_count.saturating_add(recursive_stmt_count(&function.body));
            if stmt_count > MAX_CROSS_MODULE_FUNCTION_STMTS
                || !function_shell_is_cross_module_safe(function, &allowed_ids, &mut extern_names)
                || body_references_class_in_set(&function.body, &source_class_names)
            {
                safe = false;
                break;
            }
            graph.push(function.clone());
        }
        if !safe {
            continue;
        }

        extern_names.sort();
        extern_names.dedup();
        let mut required = Vec::with_capacity(extern_names.len());
        for name in extern_names {
            let Some(import) = import_by_local.get(&name) else {
                safe = false;
                break;
            };
            required.push(import.clone());
        }
        if !safe {
            continue;
        }
        required.sort_by(|a, b| {
            (&a.resolved_path, &a.imported, &a.local).cmp(&(
                &b.resolved_path,
                &b.imported,
                &b.local,
            ))
        });
        required.dedup();
        out.insert(
            exported_name.clone(),
            FunctionCandidate {
                root_id: *root_id,
                functions: graph,
                required_extern_imports: required,
            },
        );
    }
    out
}

fn collect_function_graph(
    id: FuncId,
    functions: &HashMap<FuncId, &Function>,
    visiting: &mut HashSet<FuncId>,
    visited: &mut HashSet<FuncId>,
    out: &mut Vec<FuncId>,
) -> bool {
    if visited.contains(&id) {
        return true;
    }
    if !visiting.insert(id) {
        return false;
    }
    let Some(function) = functions.get(&id).copied() else {
        return false;
    };
    if !is_inlinable(function)
        || !function.decorators.is_empty()
        || function
            .params
            .iter()
            .any(|param| !param.decorators.is_empty() || param.arguments_object.is_some())
    {
        return false;
    }
    let mut refs = Vec::new();
    collect_func_refs_in_function(function, &mut refs);
    refs.sort_unstable();
    refs.dedup();
    for dependency in refs {
        if !collect_function_graph(dependency, functions, visiting, visited, out) {
            return false;
        }
        if visited.len() > MAX_CROSS_MODULE_FUNCTION_GRAPH {
            return false;
        }
    }
    visiting.remove(&id);
    visited.insert(id);
    out.push(id);
    true
}

fn collect_func_refs_in_function(function: &Function, out: &mut Vec<FuncId>) {
    for param in &function.params {
        if let Some(default) = &param.default {
            collect_func_refs_in_expr(default, out);
        }
    }
    collect_func_refs_in_stmts(&function.body, out);
}

fn collect_func_refs_in_expr(expr: &Expr, out: &mut Vec<FuncId>) {
    if let Expr::FuncRef(id) = expr {
        out.push(*id);
    }
    if let Expr::Closure { params, body, .. } = expr {
        for param in params {
            if let Some(default) = &param.default {
                collect_func_refs_in_expr(default, out);
            }
        }
        collect_func_refs_in_stmts(body, out);
    }
    walk_expr_children(expr, &mut |child| collect_func_refs_in_expr(child, out));
}

fn collect_func_refs_in_stmts(stmts: &[Stmt], out: &mut Vec<FuncId>) {
    walk_stmts(stmts, &mut |expr| collect_func_refs_in_expr(expr, out));
}

fn function_shell_is_cross_module_safe(
    function: &Function,
    allowed_ids: &HashSet<FuncId>,
    extern_names: &mut Vec<String>,
) -> bool {
    if !function.captures.is_empty() || !function_locals_are_self_contained(function) {
        return false;
    }
    function.params.iter().all(|param| {
        param
            .default
            .as_ref()
            .is_none_or(|default| cross_function_expr_is_safe(default, allowed_ids, extern_names))
    }) && cross_function_stmts_are_safe(&function.body, allowed_ids, extern_names)
}

/// LocalIds are meaningful only inside their defining module. A top-level
/// function can read a module binding (for example `COMPONENT_ID_MAX`) as a
/// plain LocalGet without listing it as a closure capture. Copying that body
/// into another module would silently bind the numeric id to an unrelated
/// destination local. Admit only references declared by the function itself.
fn function_locals_are_self_contained(function: &Function) -> bool {
    let mut declared: HashSet<LocalId> = function.params.iter().map(|param| param.id).collect();
    collect_declared_local_ids(&function.body, &mut declared);

    let mut refs = Vec::new();
    let mut visited = HashSet::new();
    for param in &function.params {
        if let Some(default) = &param.default {
            perry_hir::collect_local_refs_expr(default, &mut refs, &mut visited);
        }
    }
    for stmt in &function.body {
        perry_hir::collect_local_refs_stmt(stmt, &mut refs, &mut visited);
    }
    refs.into_iter().all(|id| declared.contains(&id))
}

fn cross_function_expr_is_safe(
    expr: &Expr,
    allowed_ids: &HashSet<FuncId>,
    extern_names: &mut Vec<String>,
) -> bool {
    match expr {
        Expr::FuncRef(id) => allowed_ids.contains(id),
        Expr::ExternFuncRef { name, .. } => {
            extern_names.push(name.clone());
            true
        }
        Expr::GlobalGet(_)
        | Expr::GlobalSet(_, _)
        | Expr::NativeModuleRef(_)
        // These nodes carry source-module-relative path sets. Codegen resolves
        // those sets through the current module's dynamic-target map, so a
        // clone installed in an importer would silently use the wrong map and
        // fall through to ambient require/import handling.
        | Expr::DynamicImport { .. }
        | Expr::WorkerNew { .. } => false,
        // Capture-free zero-argument closures are self-contained after their
        // FuncId is refreshed in the destination. This admits default thunks
        // such as `exists = () => true` without allowing a source local to
        // leak across the module boundary.
        Expr::Closure {
            params,
            body,
            captures,
            mutable_captures,
            captures_this,
            captures_new_target,
            ..
        } => {
            params.is_empty()
                && captures.is_empty()
                && mutable_captures.is_empty()
                && !captures_this
                && !captures_new_target
                && cross_function_stmts_are_safe(body, allowed_ids, extern_names)
        }
        other => {
            let mut safe = true;
            walk_expr_children(other, &mut |child| {
                if !cross_function_expr_is_safe(child, allowed_ids, extern_names) {
                    safe = false;
                }
            });
            safe
        }
    }
}

fn cross_function_stmts_are_safe(
    stmts: &[Stmt],
    allowed_ids: &HashSet<FuncId>,
    extern_names: &mut Vec<String>,
) -> bool {
    let mut safe = true;
    walk_stmts(stmts, &mut |expr| {
        if !cross_function_expr_is_safe(expr, allowed_ids, extern_names) {
            safe = false;
        }
    });
    safe
}

fn recursive_stmt_count(stmts: &[Stmt]) -> usize {
    fn count(stmt: &Stmt) -> usize {
        1 + match stmt {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                then_branch.iter().map(count).sum::<usize>()
                    + else_branch
                        .as_ref()
                        .map_or(0, |branch| branch.iter().map(count).sum())
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => body.iter().map(count).sum(),
            Stmt::For { init, body, .. } => {
                init.as_ref().map_or(0, |stmt| count(stmt)) + body.iter().map(count).sum::<usize>()
            }
            Stmt::Switch { cases, .. } => cases.iter().flat_map(|case| &case.body).map(count).sum(),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                body.iter().map(count).sum::<usize>()
                    + catch
                        .as_ref()
                        .map_or(0, |clause| clause.body.iter().map(count).sum())
                    + finally
                        .as_ref()
                        .map_or(0, |body| body.iter().map(count).sum())
            }
            Stmt::Labeled { body, .. } => count(body),
            _ => 0,
        }
    }
    stmts.iter().map(count).sum()
}

fn walk_stmts(stmts: &[Stmt], visit: &mut impl FnMut(&Expr)) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(expr) = init {
                    visit(expr);
                }
            }
            Stmt::Expr(expr) | Stmt::Throw(expr) | Stmt::Return(Some(expr)) => visit(expr),
            Stmt::Return(None) | Stmt::Break | Stmt::Continue => {}
            Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                visit(condition);
                walk_stmts(then_branch, visit);
                if let Some(branch) = else_branch {
                    walk_stmts(branch, visit);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                visit(condition);
                walk_stmts(body, visit);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    walk_stmts(std::slice::from_ref(init.as_ref()), visit);
                }
                if let Some(condition) = condition {
                    visit(condition);
                }
                if let Some(update) = update {
                    visit(update);
                }
                walk_stmts(body, visit);
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                visit(discriminant);
                for case in cases {
                    if let Some(test) = &case.test {
                        visit(test);
                    }
                    walk_stmts(&case.body, visit);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                walk_stmts(body, visit);
                if let Some(catch) = catch {
                    walk_stmts(&catch.body, visit);
                }
                if let Some(finally) = finally {
                    walk_stmts(finally, visit);
                }
            }
            Stmt::Labeled { body, .. } => walk_stmts(std::slice::from_ref(body.as_ref()), visit),
            Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) | Stmt::ReleaseBoxes(_) => {}
        }
    }
}

/// Install imported function candidates as private destination-local
/// functions, then retarget only direct calls to those fresh functions. The
/// private clone is a correctness fallback: if the ordinary inliner declines
/// a call site, codegen still sees a valid destination FuncId rather than a
/// source-module ID.
pub(crate) fn localize_cross_module_functions(
    module: &mut Module,
    candidates: &HashMap<(String, String), FunctionCandidate>,
) {
    if candidates.is_empty() {
        return;
    }

    type CandidateKey = (String, String);

    let mut used_locals: HashSet<String> = HashSet::new();
    let mut binding_for_import: BTreeMap<CandidateKey, String> = BTreeMap::new();
    let mut aliases_by_candidate: BTreeMap<CandidateKey, BTreeSet<String>> = BTreeMap::new();
    let mut selected = BTreeSet::new();
    let mut queue = VecDeque::new();

    for import in &module.imports {
        let Some(path) = &import.resolved_path else {
            continue;
        };
        for specifier in &import.specifiers {
            match specifier {
                ImportSpecifier::Named { imported, local } => {
                    used_locals.insert(local.clone());
                    if import.type_only {
                        continue;
                    }
                    let key = (path.clone(), imported.clone());
                    binding_for_import
                        .entry(key.clone())
                        .or_insert_with(|| local.clone());
                    if candidates.contains_key(&key) {
                        aliases_by_candidate
                            .entry(key.clone())
                            .or_default()
                            .insert(local.clone());
                        if selected.insert(key.clone()) {
                            queue.push_back(key);
                        }
                    }
                }
                ImportSpecifier::Default { local } | ImportSpecifier::Namespace { local } => {
                    used_locals.insert(local.clone());
                }
            }
        }
    }
    if selected.is_empty() {
        return;
    }
    used_locals.extend(
        module
            .functions
            .iter()
            .map(|function| function.name.clone()),
    );
    used_locals.extend(module.classes.iter().map(|class| class.name.clone()));
    used_locals.extend(module.globals.iter().map(|global| global.name.clone()));

    let mut pending_imports: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
    let mut alias_counter = 0usize;
    while let Some(key) = queue.pop_front() {
        let Some(candidate) = candidates.get(&key) else {
            continue;
        };
        for required in &candidate.required_extern_imports {
            let dependency_key = (required.resolved_path.clone(), required.imported.clone());
            let local = if let Some(local) = binding_for_import.get(&dependency_key) {
                local.clone()
            } else {
                let local = loop {
                    let candidate_local = format!(
                        "__perry_xmod_{}_{}",
                        alias_counter,
                        sanitize_identifier(&required.local)
                    );
                    alias_counter += 1;
                    if used_locals.insert(candidate_local.clone()) {
                        break candidate_local;
                    }
                };
                binding_for_import.insert(dependency_key.clone(), local.clone());
                pending_imports
                    .entry(required.resolved_path.clone())
                    .or_default()
                    .insert((required.imported.clone(), local.clone()));
                local
            };
            if candidates.contains_key(&dependency_key)
                && selected.len() < MAX_LOCALIZED_FUNCTION_ROOTS
            {
                aliases_by_candidate
                    .entry(dependency_key.clone())
                    .or_default()
                    .insert(local);
                if selected.insert(dependency_key.clone()) {
                    queue.push_back(dependency_key);
                }
            }
        }
    }

    for (path, specifiers) in pending_imports {
        if let Some(import) = module.imports.iter_mut().find(|import| {
            !import.type_only && import.resolved_path.as_deref() == Some(path.as_str())
        }) {
            for (imported, local) in specifiers {
                if !import.specifiers.iter().any(|specifier| {
                    matches!(
                        specifier,
                        ImportSpecifier::Named {
                            imported: existing_imported,
                            local: existing_local,
                        } if existing_imported == &imported && existing_local == &local
                    )
                }) {
                    import
                        .specifiers
                        .push(ImportSpecifier::Named { imported, local });
                }
            }
        } else {
            module.imports.push(perry_hir::Import {
                source: path.clone(),
                specifiers: specifiers
                    .into_iter()
                    .map(|(imported, local)| ImportSpecifier::Named { imported, local })
                    .collect(),
                is_native: false,
                module_kind: perry_hir::ModuleKind::NativeCompiled,
                resolved_path: Some(path),
                type_only: false,
                is_dynamic: false,
                is_dynamic_target: false,
                is_deferred_require: false,
                is_adopted_require: false,
            });
        }
    }

    let mut next_func_id = crate::generator::compute_max_func_id(module).saturating_add(1);
    let mut next_local_id = crate::generator::compute_max_local_id(module).saturating_add(1);
    let mut call_rewrites: HashMap<String, FuncId> = HashMap::new();
    let mut localized_functions = Vec::new();

    for key in selected {
        let Some(candidate) = candidates.get(&key) else {
            continue;
        };
        let mut func_id_remap = HashMap::new();
        for function in &candidate.functions {
            func_id_remap.insert(function.id, next_func_id);
            next_func_id = next_func_id.saturating_add(1);
        }
        let Some(&localized_root_id) = func_id_remap.get(&candidate.root_id) else {
            continue;
        };

        if let Some(aliases) = aliases_by_candidate.get(&key) {
            for alias in aliases {
                call_rewrites.insert(alias.clone(), localized_root_id);
            }
        }

        let extern_renames: HashMap<String, String> = candidate
            .required_extern_imports
            .iter()
            .filter_map(|required| {
                let dependency_key = (required.resolved_path.clone(), required.imported.clone());
                binding_for_import
                    .get(&dependency_key)
                    .map(|local| (required.local.clone(), local.clone()))
            })
            .collect();
        let mut closure_func_remap = HashMap::new();

        for source_function in &candidate.functions {
            let mut function = source_function.clone();
            let Some(&localized_id) = func_id_remap.get(&source_function.id) else {
                continue;
            };
            function.id = localized_id;
            function.name = format!(
                "__perry_xmod_inline_{}_{}",
                localized_id,
                sanitize_identifier(&source_function.name)
            );
            function.is_exported = false;

            let mut local_remap: HashMap<LocalId, Expr> = HashMap::new();
            for param in &function.params {
                local_remap.entry(param.id).or_insert_with(|| {
                    let fresh = next_local_id;
                    next_local_id = next_local_id.saturating_add(1);
                    Expr::LocalGet(fresh)
                });
            }
            for id in collect_body_local_ids(&function.body) {
                local_remap.entry(id).or_insert_with(|| {
                    let fresh = next_local_id;
                    next_local_id = next_local_id.saturating_add(1);
                    Expr::LocalGet(fresh)
                });
            }
            for param in &mut function.params {
                if let Some(Expr::LocalGet(fresh)) = local_remap.get(&param.id) {
                    param.id = *fresh;
                }
                if let Some(default) = &mut param.default {
                    substitute_locals(default, &local_remap, &mut next_local_id);
                    rewrite_candidate_expr(
                        default,
                        &func_id_remap,
                        &extern_renames,
                        &mut closure_func_remap,
                        &mut next_func_id,
                    );
                }
            }
            substitute_locals_in_stmts(&mut function.body, &local_remap, &mut next_local_id);
            rewrite_candidate_stmts(
                &mut function.body,
                &func_id_remap,
                &extern_renames,
                &mut closure_func_remap,
                &mut next_func_id,
            );
            localized_functions.push(function);
        }
    }

    module.functions.extend(localized_functions);
    rewrite_direct_extern_calls_in_module(module, &call_rewrites);
}

fn sanitize_identifier(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "fn".to_string()
    } else {
        sanitized
    }
}

fn rewrite_candidate_stmts(
    stmts: &mut [Stmt],
    func_id_remap: &HashMap<FuncId, FuncId>,
    extern_renames: &HashMap<String, String>,
    closure_func_remap: &mut HashMap<FuncId, FuncId>,
    next_func_id: &mut FuncId,
) {
    walk_stmts_mut(stmts, &mut |expr| {
        rewrite_candidate_expr(
            expr,
            func_id_remap,
            extern_renames,
            closure_func_remap,
            next_func_id,
        )
    });
}

fn rewrite_candidate_expr(
    expr: &mut Expr,
    func_id_remap: &HashMap<FuncId, FuncId>,
    extern_renames: &HashMap<String, String>,
    closure_func_remap: &mut HashMap<FuncId, FuncId>,
    next_func_id: &mut FuncId,
) {
    match expr {
        Expr::FuncRef(id) => {
            if let Some(remapped) = func_id_remap.get(id) {
                *id = *remapped;
            }
            return;
        }
        Expr::ExternFuncRef { name, .. } => {
            if let Some(remapped) = extern_renames.get(name) {
                *name = remapped.clone();
            }
            return;
        }
        Expr::Closure {
            func_id,
            params,
            body,
            ..
        } => {
            let original = *func_id;
            *func_id = *closure_func_remap.entry(original).or_insert_with(|| {
                let fresh = *next_func_id;
                *next_func_id = next_func_id.saturating_add(1);
                fresh
            });
            for param in params {
                if let Some(default) = &mut param.default {
                    rewrite_candidate_expr(
                        default,
                        func_id_remap,
                        extern_renames,
                        closure_func_remap,
                        next_func_id,
                    );
                }
            }
            rewrite_candidate_stmts(
                body,
                func_id_remap,
                extern_renames,
                closure_func_remap,
                next_func_id,
            );
            return;
        }
        _ => {}
    }
    walk_expr_children_mut(expr, &mut |child| {
        rewrite_candidate_expr(
            child,
            func_id_remap,
            extern_renames,
            closure_func_remap,
            next_func_id,
        )
    });
}

fn rewrite_direct_extern_calls_in_module(
    module: &mut Module,
    call_rewrites: &HashMap<String, FuncId>,
) {
    let rewrite_body = |body: &mut [Stmt]| {
        walk_stmts_mut(body, &mut |expr| {
            rewrite_direct_extern_call(expr, call_rewrites)
        });
    };
    rewrite_body(&mut module.init);
    for function in &mut module.functions {
        rewrite_body(&mut function.body);
    }
    for class in &mut module.classes {
        if let Some(constructor) = &mut class.constructor {
            rewrite_body(&mut constructor.body);
        }
        for method in &mut class.methods {
            rewrite_body(&mut method.body);
        }
        for (_, getter) in &mut class.getters {
            rewrite_body(&mut getter.body);
        }
        for (_, setter) in &mut class.setters {
            rewrite_body(&mut setter.body);
        }
        for method in &mut class.static_methods {
            rewrite_body(&mut method.body);
        }
        for member in &mut class.computed_members {
            rewrite_body(&mut member.function.body);
        }
    }
}

fn rewrite_direct_extern_call(expr: &mut Expr, call_rewrites: &HashMap<String, FuncId>) {
    if let Expr::Call { callee, .. } = expr {
        if let Expr::ExternFuncRef { name, .. } = callee.as_ref() {
            if let Some(id) = call_rewrites.get(name) {
                **callee = Expr::FuncRef(*id);
            }
        }
    }
    if let Expr::Closure { body, .. } = expr {
        walk_stmts_mut(body, &mut |child| {
            rewrite_direct_extern_call(child, call_rewrites)
        });
    }
    walk_expr_children_mut(expr, &mut |child| {
        rewrite_direct_extern_call(child, call_rewrites)
    });
}

fn walk_stmts_mut(stmts: &mut [Stmt], visit: &mut impl FnMut(&mut Expr)) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(expr) = init {
                    visit(expr);
                }
            }
            Stmt::Expr(expr) | Stmt::Throw(expr) | Stmt::Return(Some(expr)) => visit(expr),
            Stmt::Return(None) | Stmt::Break | Stmt::Continue => {}
            Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                visit(condition);
                walk_stmts_mut(then_branch, visit);
                if let Some(branch) = else_branch {
                    walk_stmts_mut(branch, visit);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                visit(condition);
                walk_stmts_mut(body, visit);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    walk_stmts_mut(std::slice::from_mut(init.as_mut()), visit);
                }
                if let Some(condition) = condition {
                    visit(condition);
                }
                if let Some(update) = update {
                    visit(update);
                }
                walk_stmts_mut(body, visit);
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                visit(discriminant);
                for case in cases {
                    if let Some(test) = &mut case.test {
                        visit(test);
                    }
                    walk_stmts_mut(&mut case.body, visit);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                walk_stmts_mut(body, visit);
                if let Some(catch) = catch {
                    walk_stmts_mut(&mut catch.body, visit);
                }
                if let Some(finally) = finally {
                    walk_stmts_mut(finally, visit);
                }
            }
            Stmt::Labeled { body, .. } => {
                walk_stmts_mut(std::slice::from_mut(body.as_mut()), visit)
            }
            Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) | Stmt::ReleaseBoxes(_) => {}
        }
    }
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
            // Conservative: a body carrying a box-release hint is an async
            // step machine; never harvest it for cross-module inlining.
            Stmt::ReleaseBoxes(_) => false,
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
            Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) | Stmt::ReleaseBoxes(_) => {
                false
            }
        }
    }
    stmts.iter().any(|s| check_stmt(s, set))
}
