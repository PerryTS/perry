//! Collection-time, monotone export demand. Ordinary imports are retained. An
//! unused re-export is postponed only when its entire static dependency tree
//! is covered by package sideEffects contracts or inert-initialization proofs.
//! Later importers can reactivate it; HIR edges are removed only after every
//! collection root has settled.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use perry_hir::{Export, Import, ImportSpecifier};

use super::super::CompilationContext;

mod forwarding;
mod purity;
mod scan;
mod side_effects;
pub(super) use forwarding::normalize as normalize_forwarding_barrel;

#[derive(Clone, Default)]
struct Demand {
    all: bool,
    names: HashSet<String>,
}

impl Demand {
    fn all() -> Self {
        Self {
            all: true,
            names: HashSet::new(),
        }
    }

    fn name(name: &str) -> Self {
        Self {
            all: false,
            names: HashSet::from([name.to_owned()]),
        }
    }

    fn contains(&self, name: &str) -> bool {
        self.all || self.names.contains(name)
    }
}

struct Edge {
    from: PathBuf,
    index: usize,
    target: PathBuf,
    source_path: PathBuf,
    export: Export,
    safe: bool,
    active: bool,
}

#[derive(Default)]
pub(crate) struct ReexportPruner {
    demands: HashMap<PathBuf, Demand>,
    edges: Vec<Edge>,
    scan: scan::Scanner,
    pub(crate) pruned: usize,
}

fn enabled() -> bool {
    std::env::var("PERRY_NO_REEXPORT_PRUNE").ok().as_deref() != Some("1")
}

impl ReexportPruner {
    fn demand(&mut self, path: &Path, incoming: Demand) -> bool {
        let current = self.demands.entry(path.to_owned()).or_default();
        if current.all {
            return false;
        }
        if incoming.all {
            *current = incoming;
            return true;
        }
        let before = current.names.len();
        current.names.extend(incoming.names);
        current.names.len() != before
    }

    pub(crate) fn root(&mut self, path: &Path) {
        self.demand(path, Demand::all());
    }

    pub(super) fn implicit_root(&mut self, path: &Path) {
        if !self.demands.contains_key(path) {
            self.root(path);
        }
    }

    pub(crate) fn imports(&mut self, imports: &[Import]) {
        if !enabled() {
            return;
        }
        for import in imports {
            if import.type_only || import.runtime_erased {
                continue;
            }
            let Some(path) = &import.resolved_path else {
                continue;
            };
            let mut demand = Demand::default();
            // A dynamic import exposes the complete namespace, even when the
            // same source also has a named static import. require interop and
            // namespaces likewise cannot be narrowed to named imports.
            demand.all = import.is_dynamic || import.is_dynamic_target || import.is_adopted_require;
            for spec in &import.specifiers {
                match spec {
                    ImportSpecifier::Named { imported, .. } => {
                        demand.names.insert(imported.clone());
                    }
                    ImportSpecifier::Default { .. } => {
                        demand.names.insert("default".into());
                    }
                    ImportSpecifier::Namespace { .. } => demand.all = true,
                }
            }
            self.demand(Path::new(path), demand);
        }
    }

    fn forwarded(&mut self, edge: &Edge, ctx: &mut CompilationContext) -> Option<Demand> {
        let needed = self.demands.get(&edge.from).cloned().unwrap_or_default();
        // An effectful edge retains its complete original export surface, so
        // namespace getters cannot refer to missing transitive bindings.
        let force = !edge.safe || edge.from == edge.target;
        match &edge.export {
            Export::ReExport {
                imported, exported, ..
            } => (force || needed.contains(exported)).then(|| Demand::name(imported)),
            Export::NamespaceReExport { name, .. } => {
                (force || needed.contains(name)).then(Demand::all)
            }
            Export::ExportAll { .. } => {
                if force {
                    return Some(Demand::all());
                }
                if needed.all {
                    // `export *` forwards neither default nor erased TS
                    // declarations, even to a consumer of the full namespace.
                    return self
                        .scan
                        .may_have_star_exports(&edge.source_path, ctx)
                        .then(Demand::all);
                }
                let names: HashSet<_> = needed
                    .names
                    .into_iter()
                    .filter(|name| {
                        name != "default" && self.scan.might_export(&edge.source_path, name, ctx)
                    })
                    .collect();
                (!names.is_empty()).then_some(Demand { all: false, names })
            }
            Export::Named { .. } => None,
        }
    }
}

/// Return whether the ordinary DFS should visit this target immediately.
pub(super) fn record(
    ctx: &mut CompilationContext,
    from: &Path,
    from_source: &Path,
    index: usize,
    export: &Export,
    target: &Path,
    source_path: &Path,
) -> bool {
    if !enabled() {
        return true;
    }
    let mut state = std::mem::take(&mut ctx.reexport_pruner);
    let safe =
        state.scan.module_is_pure(from_source, ctx) && state.scan.can_drop_tree(source_path, ctx);
    let mut edge = Edge {
        from: from.to_owned(),
        index,
        target: target.to_owned(),
        source_path: source_path.to_owned(),
        export: export.clone(),
        safe,
        active: false,
    };
    if let Some(demand) = state.forwarded(&edge, ctx) {
        state.demand(target, demand);
        edge.active = true;
    }
    let active = edge.active;
    state.edges.push(edge);
    ctx.reexport_pruner = state;
    active
}

/// Revisit postponed edges after later direct/dynamic imports add demand.
/// This also forwards new names across already-active barrel edges. Demand
/// only grows, so cycles converge without depending on DFS visitation order.
pub(super) fn settle(ctx: &mut CompilationContext) -> Vec<PathBuf> {
    let mut state = std::mem::take(&mut ctx.reexport_pruner);
    let mut edges = std::mem::take(&mut state.edges);
    let mut pending = Vec::new();
    loop {
        let mut changed = false;
        for edge in &mut edges {
            if let Some(demand) = state.forwarded(edge, ctx) {
                changed |= state.demand(&edge.target, demand);
                if !edge.active {
                    edge.active = true;
                    pending.push(edge.source_path.clone());
                }
            }
        }
        if !changed {
            break;
        }
    }
    state.edges = edges;
    ctx.reexport_pruner = state;
    pending
}

pub(crate) fn finish(ctx: &mut CompilationContext) {
    let mut state = std::mem::take(&mut ctx.reexport_pruner);
    let mut removed: HashMap<PathBuf, HashSet<usize>> = HashMap::new();
    let mut omitted = HashSet::new();
    for edge in &state.edges {
        if !edge.active {
            removed
                .entry(edge.from.clone())
                .or_default()
                .insert(edge.index);
            state.scan.static_tree(&edge.source_path, &mut omitted);
        }
    }
    for (path, indices) in removed {
        if let Some(module) = ctx.native_modules.get_mut(&path) {
            let mut index = 0;
            module.exports.retain(|_| {
                let keep = !indices.contains(&index);
                index += 1;
                keep
            });
        }
    }
    state.pruned = omitted
        .iter()
        .filter(|p| !ctx.native_modules.contains_key(*p))
        .count();
    ctx.reexport_pruner = state;
}

pub(crate) fn write_graph(ctx: &mut CompilationContext, entry: &Path) -> anyhow::Result<()> {
    super::super::init_order::classify_eager_modules(ctx, entry);
    let modules: Vec<_> = ctx.native_modules.iter().map(|(path, module)| {
        serde_json::json!({
            "path": path,
            "init": if module.init_kind == perry_hir::ModuleInitKind::Eager { "eager" } else { "deferred" },
        })
    }).collect();
    let graph = serde_json::json!({"modules": modules, "pruned": ctx.reexport_pruner.pruned});
    std::fs::write(
        ctx.cache_dir.join("module-graph.json"),
        serde_json::to_vec_pretty(&graph)?,
    )?;
    Ok(())
}
