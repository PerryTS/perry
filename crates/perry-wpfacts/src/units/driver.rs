use super::*;

/// One solve of the per-unit driver: a declaration unit of module `mi`
/// (design §12.6), its view, the solves it depends on (the units of its
/// module it names, every unit of each module it imports: what a census
/// round solves again), and the solves it is ordered after.
pub(super) struct Solve<'a> {
    mi: usize,
    key: String,
    /// The one module the unit is cut from (its run-once view, N5, when
    /// it has one): borrowed, never copied per unit.
    module: &'a Module,
    view: crate::decl_units::UnitView<'a>,
    deps: Vec<usize>,
    /// Decision 21: the units of its module it names and, in each module it
    /// imports, the units owning the names it refers to (every unit for a
    /// namespace import or a name no unit exports). A unit reads another
    /// module's contracts only through those names, so this is the order
    /// the contracts must exist in; ordering by whole modules made cycles
    /// the names do not have, and a back edge's importer sees ⊤.
    after: Vec<usize>,
}

/// The unit of module range `t` exporting `name`; a re-export goes with
/// the first unit (`decl_units::views`).
/// The unit of module range `t` that exports `name` as an export of the
/// module: a unit exporting it to the module's own units only
/// (`UnitView::unit_exports`) is not the module's binding of that name
/// (`var _C = class _C {..}; export { _C }`: the class unit exports `_C` to
/// the binding's unit, which the module exports).
pub(super) fn owner_of(out: &[Solve], t: &std::ops::Range<usize>, name: &str) -> Option<usize> {
    t.clone().find(|&si| {
        !out[si].view.unit_exports.contains(name)
            && out[si].view.exports.iter().any(|e| match e {
                Export::Named { exported, .. } | Export::ReExport { exported, .. } => {
                    exported == name
                }
                Export::NamespaceReExport { name: n, .. } => n == name,
                _ => false,
            })
    })
}

pub(super) fn solves<'a>(
    sorted: &[(&'a str, &'a Module)],
    flats: &'a [Option<Module>],
    cfg: &FactsConfig,
    ex: &Exports,
) -> Vec<Solve<'a>> {
    // DECISIONS 129: what other modules can name of each module, from the
    // modules' own imports and re-exports (a unit's imports of its own
    // module's units are not another module's).
    let own: Vec<(&str, Vec<&Import>, &[Export])> = sorted
        .iter()
        .map(|(p, m)| (*p, m.imports.iter().collect(), m.exports.as_slice()))
        .collect();
    let reach = published_surface(&own, cfg, ex);
    let index: FxHashMap<&str, usize> = sorted
        .iter()
        .enumerate()
        .map(|(i, (p, _))| (*p, i))
        .collect();
    let mut out: Vec<Solve<'a>> = Vec::new();
    let mut range: Vec<std::ops::Range<usize>> = Vec::with_capacity(sorted.len());
    for (mi, (path, module)) in sorted.iter().enumerate() {
        let base = out.len();
        // A body that runs once at a module-scope statement is module scope
        // for the cut (N5, §12.5): the module's run-once view, one per
        // module, shared by all its units.
        let module: &'a Module = flats[mi].as_ref().unwrap_or(module);
        let named = |n: &str| reach.eval_scopes || reach.names(path, n);
        for (k, (view, deps)) in crate::decl_units::views(path, module, &named)
            .into_iter()
            .enumerate()
        {
            let deps: Vec<usize> = deps.into_iter().map(|d| base + d).collect();
            out.push(Solve {
                mi,
                key: crate::decl_units::unit_path(path, k),
                module,
                view: crate::decl_units::UnitView {
                    key: crate::decl_units::unit_path(path, k),
                    ..view
                },
                after: deps.clone(),
                deps,
            });
        }
        range.push(base..out.len());
    }
    let mut ns_reach: FxHashMap<usize, Vec<usize>> = FxHashMap::default();
    for si in 0..out.len() {
        let (path, module) = sorted[out[si].mi];
        let mut after: Vec<usize> = Vec::new();
        let mut deps: Vec<usize> = Vec::new();
        for imp in &module.imports {
            let Some(&t) = imp
                .resolved_path
                .as_deref()
                .filter(|t| *t != path)
                .and_then(|t| index.get(t))
            else {
                continue;
            };
            out[si].deps.extend(range[t].clone());
            // A re-export's names come from its source: the first unit,
            // which holds the module's re-exports, is ordered after all of it.
            let reexports = out[si].key.ends_with("#u0")
                && module.exports.iter().any(|e| match e {
                    Export::ReExport { source, .. }
                    | Export::ExportAll { source }
                    | Export::NamespaceReExport { source, .. } => *source == imp.source,
                    _ => false,
                });
            if reexports {
                after.extend(range[t].clone());
            }
            for sp in &imp.specifiers {
                let (local, name) = match sp {
                    ImportSpecifier::Named { imported, local } => (local, Some(imported.as_str())),
                    ImportSpecifier::Default { local } => (local, Some("default")),
                    ImportSpecifier::Namespace { local } => (local, None),
                };
                if !out[si].view.ext_names.contains(local) {
                    continue;
                }
                // A name another module re-exports is read where it is
                // declared (§18.14): ordered after, and depending on, the
                // declaring unit, not the re-exporting module (whose units
                // are ordered after all of their sources, a cycle when the
                // declaring module imports the re-exporter too).
                let (d, name) = match name.and_then(|n| ex.resolve(sorted[t].0, n)) {
                    Some(Resolved::Binding(p, n)) => (index[p], Some(n)),
                    Some(Resolved::Namespace(p)) => (index[p], None),
                    _ => (t, name),
                };
                if d != t {
                    deps.extend(range[d].clone());
                }
                // A namespace's keys are read where they are declared: its
                // unit is ordered after those modules too.
                if name.is_none() {
                    for &r in ns_reach.entry(d).or_insert_with(|| ex.reach(d)).iter() {
                        if r != out[si].mi {
                            deps.extend(range[r].clone());
                            after.extend(range[r].clone());
                        }
                    }
                }
                match name.and_then(|n| owner_of(&out, &range[d], n)) {
                    Some(w) => after.push(w),
                    None => after.extend(range[d].clone()),
                }
            }
        }
        // A re-export whose source the module does not import (resolved
        // by the driver): the first unit is ordered after all of the source
        // and depends on it, as for an imported one.
        if out[si].key.ends_with("#u0") {
            for e in &module.exports {
                let (Export::ReExport { source, .. }
                | Export::ExportAll { source }
                | Export::NamespaceReExport { source, .. }) = e
                else {
                    continue;
                };
                if module.imports.iter().any(|i| &i.source == source) {
                    continue;
                }
                let Some(&t) =
                    reexport_source(&cfg.export_sources, path, std::iter::empty(), source)
                        .as_deref()
                        .filter(|t| *t != path)
                        .and_then(|t| index.get(t))
                else {
                    continue;
                };
                out[si].deps.extend(range[t].clone());
                after.extend(range[t].clone());
            }
        }
        // Each once, in first-named order (the order's DFS follows it).
        let mut once: FxHashSet<usize> = FxHashSet::default();
        after.retain(|&w| once.insert(w));
        out[si].after.extend(after);
        deps.sort_unstable();
        deps.dedup();
        out[si].deps.extend(deps);
    }
    out
}

/// Solves in dependency order (a solve after the ones it depends on), so a
/// contract is computed before a unit consumes it. In an import cycle the
/// back edge has no contract yet: that import is ⊤, as in P1.
pub(super) fn dependency_order(solves: &[Solve]) -> Vec<usize> {
    let mut state = vec![0u8; solves.len()];
    let mut out = Vec::with_capacity(solves.len());
    for root in 0..solves.len() {
        if state[root] != 0 {
            continue;
        }
        // Iterative DFS, post-order.
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        state[root] = 1;
        while let Some(&mut (v, ref mut next)) = stack.last_mut() {
            if *next < solves[v].after.len() {
                let w = solves[v].after[*next];
                *next += 1;
                if state[w] == 0 {
                    state[w] = 1;
                    stack.push((w, 0));
                }
            } else {
                state[v] = 2;
                out.push(v);
                stack.pop();
            }
        }
    }
    out
}

/// The per-unit driver (decision 151, design §7, §11 and §12): one solve
/// per declaration unit in dependency order, each publishing contracts to
/// the units after it; then the census. A class some unit used beyond its
/// contract is broken: it is published as an escape, and the units that see
/// it again (its defining unit and every unit depending on that one,
/// transitively: contracts travel only along those edges) are solved again;
/// every other unit's inputs are unchanged, so its result is kept. What
/// the other units feed back only grows: broken, the argument contracts
/// (decision 13) and the store effects (decision 17), each last round's
/// value joined with what the latest solves report (D16): the entries
/// (templates, functions, keys, ⊤) and their descriptions are joins in
/// finite lattices (`ArgContract::join`), so every such input ascends and
/// the loop ends, and the result covers what every round's solves passed
/// and stored.

pub(crate) fn run(sorted: &[(&str, &Module)], cfg: &FactsConfig) -> ProgramFacts {
    let ucfg = FactsConfig {
        unit: true,
        ..cfg.clone()
    };
    let flats: Vec<Option<Module>> = sorted
        .iter()
        .map(|(_, m)| crate::decl_units::run_once_flat(m))
        .collect();
    let exports = module_exports(sorted, &cfg.export_sources);
    let solves = solves(sorted, &flats, cfg, &exports);
    let viewed: Vec<(&str, Vec<&Import>, &[Export])> = solves
        .iter()
        .map(|s| {
            (
                sorted[s.mi].0,
                s.module
                    .imports
                    .iter()
                    .chain(s.view.imports.iter())
                    .collect(),
                s.view.exports.as_slice(),
            )
        })
        .collect();
    let mut input = published_surface(&viewed, cfg, &exports);
    input.exports = exports;
    // The classes another unit's class extends, through the import its
    // `extends` names (DECISIONS 57).
    for s in &solves {
        for &k in &s.view.classes {
            let Some(e) = s
                .module
                .classes
                .get(k)
                .and_then(|c| c.extends_name.as_ref())
            else {
                continue;
            };
            for imp in s.module.imports.iter().chain(s.view.imports.iter()) {
                let Some(t) = imp.resolved_path.as_deref() else {
                    continue;
                };
                for sp in &imp.specifiers {
                    let name = match sp {
                        ImportSpecifier::Named { imported, local } if local == e => {
                            imported.clone()
                        }
                        ImportSpecifier::Default { local } if local == e => "default".into(),
                        _ => continue,
                    };
                    input
                        .extended
                        .insert((crate::decl_units::module_of(t).to_string(), name));
                }
            }
        }
    }
    let order = dependency_order(&solves);
    let levels = dependency_levels(&solves, &order);
    let mut rounds = 0u32;
    let compares = std::cell::Cell::new(0u64);
    let mut round_work: Vec<u64> = Vec::new();
    let mut per_unit: Vec<Option<ProgramFacts>> = (0..solves.len()).map(|_| None).collect();
    let mut dirty = vec![true; solves.len()];
    // DECISIONS 170 (81 (b)): a unit reads its dependencies' class,
    // function and value contracts as they stand when its level solves:
    // those of an earlier level (the others are not published yet in the
    // round, `input.classes` starts empty). `exports`: each unit's, as a
    // number per latest solve; `exports_seen`: what a unit saw of them when
    // it was solved. An importer is solved again when that changed, not
    // because a dependency was solved: equal inputs repeat the same solve.
    let mut level_of = vec![0usize; solves.len()];
    for (l, level) in levels.iter().enumerate() {
        for &si in level {
            level_of[si] = l;
        }
    }
    let visible = |si: usize, exports: &[u64]| -> Vec<u64> {
        solves[si]
            .deps
            .iter()
            .filter(|&&d| level_of[d] < level_of[si])
            .map(|&d| exports[d])
            .collect()
    };
    let mut exports: Vec<u64> = vec![0; solves.len()];
    let mut exports_seen: Vec<Vec<u64>> = vec![Vec::new(); solves.len()];
    // The layouts whose entry is their owner's, not a forwarded copy
    // (DECISIONS 106).
    let mut owned_layouts: FxHashSet<String> = FxHashSet::default();
    // The function contracts each unit published at its latest solve
    // (DECISIONS 134).
    let mut fns_of: Vec<Vec<String>> = vec![Vec::new(); solves.len()];
    // The layouts each unit published as their owner at its latest solve.
    let mut layouts_of: Vec<Vec<String>> = vec![Vec::new(); solves.len()];
    // The unit whose forwarded copy an entry is (DECISIONS 134).
    let mut copy_of: FxHashMap<String, usize> = FxHashMap::default();
    // The shared objects' writes each round replayed (`Published::shared`),
    // and the one each unit's latest solve saw.
    let mut shared_rounds: Vec<std::collections::BTreeSet<crate::model::SharedWrite>> =
        vec![Default::default()];
    let mut shared_seen: Vec<usize> = vec![0; solves.len()];
    loop {
        rounds += 1;
        input.classes.clear();
        input.fns.clear();
        input.values.clear();
        // Layouts stay from the last round (DECISIONS 103): a contract a
        // cell carries names its layouts by display, and their publisher
        // may solve after the reader: each reader recorded what it read.
        let mut published_layouts: FxHashSet<String> = FxHashSet::default();
        let mut owned_now: FxHashSet<String> = FxHashSet::default();
        let mut published_fns: FxHashSet<String> = FxHashSet::default();
        let mut reports: FxHashSet<String> = FxHashSet::default();
        let mut work = 0u64;
        for level in &levels {
            // A unit over the bound keeps its result (`dependents`).
            for &si in level {
                if !dirty[si]
                    && per_unit[si].as_ref().is_some_and(|u| !u.stats.aborted)
                    && visible(si, &exports) != exports_seen[si]
                {
                    dirty[si] = true;
                }
            }
            let todo: Vec<usize> = level.iter().copied().filter(|&si| dirty[si]).collect();
            for &si in &todo {
                exports_seen[si] = visible(si, &exports);
            }
            let mut level_max = (0usize, 0u64, 0usize);
            for (si, mut u) in solve_level(&todo, &solves, sorted, &ucfg, &input) {
                work += u.stats.work;
                level_max.0 = level_max.0.max(u.stats.nodes);
                level_max.1 = level_max.1.max(u.stats.pts_entries);
                level_max.2 = level_max.2.max(u.sites.len());
                exports[si] = digest(&(&u.contracts_out, &u.fn_contracts_out, &u.values_out));
                // DECISIONS 197: a unit's write into a cell is its last
                // write joined with what its latest solve writes, as its
                // argument contracts are (D16): a cell only grows, and a
                // value one writer took from another's earlier write cannot
                // pass back and forth between them after its source stopped
                // writing it (tsc typescript.js#u4336/#u4339, period 2).
                if let Some(old) = per_unit[si].take() {
                    let at: FxHashMap<String, usize> = u
                        .cells_out
                        .iter()
                        .enumerate()
                        .map(|(i, (k, _))| (k.clone(), i))
                        .collect();
                    for (k, c) in old.cells_out {
                        match at.get(&k) {
                            Some(&i) => u.cells_out[i].1.join(&c),
                            None => u.cells_out.push((k, c)),
                        }
                    }
                }
                per_unit[si] = Some(u);
                shared_seen[si] = shared_rounds.len() - 1;
            }
            if cfg.report && !todo.is_empty() {
                eprintln!(
                    "perry: wpfacts census r{rounds} level {}/{} solved {} max nodes {} pts {} sites {} {}",
                    level_of[todo[0]],
                    levels.len(),
                    todo.len(),
                    level_max.0,
                    level_max.1,
                    level_max.2,
                    census(&per_unit, &input, &shared_rounds)
                );
            }
            // The level's contracts, published in `order` sequence: the
            // next level's one input.
            for &si in level {
                let s = &solves[si];
                let u = per_unit[si].as_ref().expect("every unit solved");
                // Published under the unit's path (the module's other units
                // import it) and, when it is an export of the module, under
                // the module's (other modules import it). A name exported to
                // the module's units only is no export of the module: under
                // the module's path it would stand for the module's binding
                // of that name, another unit's (`var _C = class _C {..};
                // export { _C }`: the module exports the binding, the class
                // `_C` goes to the binding's unit only).
                let module_export = |n: &String| !s.view.unit_exports.contains(n);
                for ((p, n), c) in &u.contracts_out {
                    input.classes.insert((s.key.clone(), n.clone()), c.clone());
                    if module_export(n) {
                        input.classes.insert((p.clone(), n.clone()), c.clone());
                    }
                }
                for ((p, n), c) in &u.fn_contracts_out {
                    input.fns.insert((s.key.clone(), n.clone()), c.clone());
                    if module_export(n) {
                        input.fns.insert((p.clone(), n.clone()), c.clone());
                    }
                }
                for ((p, n), c) in &u.values_out {
                    input.values.insert((s.key.clone(), n.clone()), c.clone());
                    if module_export(n) {
                        input.values.insert((p.clone(), n.clone()), c.clone());
                    }
                }
                // DECISIONS 134: a layout this unit owned at its last solve
                // and no longer publishes is gone now, unless another unit
                // births it too (DECISIONS 112).
                let now: Vec<String> = u
                    .layouts_out
                    .iter()
                    .filter(|c| !u.layouts_fwd.contains(&c.display))
                    .map(|c| c.display.clone())
                    .collect();
                for d in std::mem::replace(&mut layouts_of[si], now) {
                    if !layouts_of.iter().any(|v| v.contains(&d)) {
                        input.layouts.remove(&d);
                        owned_layouts.remove(&d);
                        copy_of.remove(&d);
                    }
                }
                for c in &u.layouts_out {
                    published_layouts.insert(c.display.clone());
                    if u.layouts_fwd.contains(&c.display) {
                        if !owned_layouts.contains(&c.display) {
                            input
                                .layouts
                                .insert(c.display.clone(), Digested::new(c.clone()));
                            copy_of.insert(c.display.clone(), si);
                        }
                        continue;
                    }
                    owned_now.insert(c.display.clone());
                    copy_of.remove(&c.display);
                    // Several units birth one literal shape (DECISIONS 112):
                    // its entry is the join of their contracts, ascending
                    // from the last round's owned entry, so a reader between
                    // two owners reads at the fixpoint what the last one
                    // leaves. A class has one defining unit: its latest solve.
                    let e = match input.layouts.get(&c.display).map(|e| &e.c) {
                        Some(p)
                            if c.literal_shape
                                && p.literal_shape
                                && owned_layouts.contains(&c.display) =>
                        {
                            let mut j = p.clone();
                            j.join(c);
                            j
                        }
                        _ => c.clone(),
                    };
                    owned_layouts.insert(c.display.clone());
                    input.layouts.insert(c.display.clone(), Digested::new(e));
                }
                // DECISIONS 134: a contract this unit published at its last
                // solve and no longer publishes is gone now, not at the end
                // of the round: a reader solving later in this round reads
                // what this solve left, and needs no round to see it go.
                let now: Vec<String> = u.own_fns_out.iter().map(|c| c.name.clone()).collect();
                for nm in std::mem::replace(&mut fns_of[si], now) {
                    if !u.own_fns_out.iter().any(|c| c.name == nm) {
                        input.own_fns.remove(&nm);
                    }
                }
                for c in &u.own_fns_out {
                    published_fns.insert(c.name.clone());
                    input
                        .own_fns
                        .insert(c.name.clone(), Digested::new(c.clone()));
                }
                reports.extend(u.reports.iter().cloned());
            }
        }
        round_work.push(work);
        // Decision 13: the argument contracts of closed functions: last
        // round's joined with what every unit's latest solve passes (D16).
        // Rebuilt from the latest solves alone, a contract can shrink: a
        // closed function whose argument is ⊤ makes a caller pass less,
        // the contract drops, and the cycle repeats (express, ee-first
        // `callback`, period 3). Joined, it only grows. A closed function
        // whose contract grew is solved again, with the units depending
        // on it; at the fixpoint every closed function's parameters hold
        // what its calls pass.
        let mut latest: FxHashMap<(String, String), ArgContract> = FxHashMap::default();
        let mut aborted = false;
        for (si, s) in solves.iter().enumerate() {
            let path = sorted[s.mi].0;
            let u = per_unit[si].as_ref().expect("every unit solved");
            if u.stats.aborted {
                aborted = true;
                for nm in &s.view.closed_refs {
                    latest
                        .entry((path.to_string(), nm.clone()))
                        .or_default()
                        .top = true;
                }
                continue;
            }
            for (nm, ac) in &u.args_out {
                latest
                    .entry((path.to_string(), nm.clone()))
                    .or_default()
                    .join(ac);
            }
            // A closed class's constructions, from any module (§18.13).
            for (k, ac) in &u.ctor_args_out {
                latest.entry(k.clone()).or_default().join(ac);
            }
        }
        // DECISIONS 157: an aborted unit may hold any published class (it
        // need not name it: a namespace, a value), and its constructions
        // publish nothing.
        if aborted {
            for s in &solves {
                let path = sorted[s.mi].0;
                for nm in &s.view.published {
                    latest
                        .entry((path.to_string(), nm.clone()))
                        .or_default()
                        .top = true;
                }
            }
        }
        let mut args = input.args.clone();
        for (k, ac) in &latest {
            args.entry(k.clone()).or_default().join(ac);
        }
        let mut seeds: Vec<usize> = Vec::new();
        for (si, s) in solves.iter().enumerate() {
            let path = sorted[s.mi].0;
            let changed = s.view.closed.keys().any(|nm| {
                let k = (path.to_string(), nm.clone());
                args.get(&k) != input.args.get(&k)
            });
            if changed {
                seeds.push(si);
            }
        }
        input.args = args;
        // DECISIONS 107: the receivers of published methods: last round's
        // joined with every unit's latest solve (D16), so they only grow.
        // The owners of a method whose join changed are solved again.
        let mut this_args = input.this_args.clone();
        let mut this_top = input.this_args_top;
        for u in per_unit.iter().flatten() {
            if u.stats.aborted {
                this_top = true;
                continue;
            }
            for (nm, vc) in &u.this_args_out {
                this_args.entry(nm.clone()).or_default().join(vc);
            }
        }
        let mut keys: Vec<&String> = this_args.keys().chain(input.this_args.keys()).collect();
        keys.sort_unstable();
        keys.dedup();
        let recv_changed: Vec<String> = keys
            .into_iter()
            .filter(|k| {
                this_top != input.this_args_top
                    || digest(&this_args.get(*k)) != digest(&input.this_args.get(*k))
            })
            .map(|k| format!("fn:{k}"))
            .collect();
        if !recv_changed.is_empty() {
            for (si, u) in per_unit.iter().enumerate() {
                let u = u.as_ref().expect("every unit solved");
                if !u.stats.aborted && u.fns_defined.iter().any(|d| recv_changed.contains(d)) {
                    seeds.push(si);
                }
            }
        }
        input.this_args = this_args;
        input.this_args_top = this_top;
        // DECISIONS 165: the published methods some unit runs, joined the
        // same way; the owners of a newly run one solve again.
        let mut runs_changed: Vec<String> = Vec::new();
        for u in per_unit.iter().flatten() {
            for nm in &u.method_runs_out {
                if input.method_runs.insert(nm.clone()) {
                    runs_changed.push(format!("fn:{nm}"));
                }
            }
        }
        if !runs_changed.is_empty() {
            for (si, u) in per_unit.iter().enumerate() {
                let u = u.as_ref().expect("every unit solved");
                if !u.stats.aborted && u.fns_defined.iter().any(|d| runs_changed.contains(d)) {
                    seeds.push(si);
                }
            }
        }
        // DECISIONS 148: what the running units pass to published methods,
        // last round's joined with every unit's latest solve and compared
        // as values; the owners of a method whose join changed solve again.
        let mut margs = input.method_args.clone();
        for u in per_unit.iter().flatten() {
            for (nm, ac) in &u.method_args_out {
                margs.entry(nm.clone()).or_default().join(ac);
            }
        }
        let margs_changed: Vec<String> = margs
            .iter()
            .filter(|(k, v)| input.method_args.get(*k) != Some(*v))
            .map(|(k, _)| format!("fn:{k}"))
            .collect();
        if !margs_changed.is_empty() {
            for (si, u) in per_unit.iter().enumerate() {
                let u = u.as_ref().expect("every unit solved");
                if !u.stats.aborted && u.fns_defined.iter().any(|d| margs_changed.contains(d)) {
                    seeds.push(si);
                }
            }
        }
        input.method_args = margs;
        // DECISIONS 210: what the units constructing published functions
        // pass, joined the same way; the owners of a function whose join
        // changed solve again (they construct it with the union).
        let mut news = input.ctor_runs.clone();
        for u in per_unit.iter().flatten() {
            for (nm, ac) in &u.ctor_runs_out {
                news.entry(nm.clone()).or_default().join(ac);
            }
        }
        let news_changed: Vec<String> = news
            .iter()
            .filter(|(k, v)| input.ctor_runs.get(*k) != Some(*v))
            .map(|(k, _)| format!("fn:{k}"))
            .collect();
        if !news_changed.is_empty() {
            for (si, u) in per_unit.iter().enumerate() {
                let u = u.as_ref().expect("every unit solved");
                if !u.stats.aborted && u.fns_defined.iter().any(|d| news_changed.contains(d)) {
                    seeds.push(si);
                }
            }
        }
        input.ctor_runs = news;
        // Decision 17: the store effects: last round's joined with what
        // every unit's latest solve stores (D16), so they only grow too.
        // The owners of a layout whose effect keys grew perform them,
        // solved again with their dependents.
        let mut stores = input.stores.clone();
        for u in per_unit.iter().flatten() {
            for (d, k, c) in &u.stores_out {
                let v = stores.entry(d.clone()).or_default();
                match v.iter_mut().find(|(k2, _)| k2 == k) {
                    Some((_, e)) => e.join(c),
                    None => v.push((k.clone(), c.clone())),
                }
            }
        }
        for v in stores.values_mut() {
            v.sort_by(|x, y| x.0.cmp(&y.0));
        }
        // Compared as values: a layout whose keys differ either way (joined,
        // they can only have grown).
        let mut changed: Vec<String> = stores
            .keys()
            .chain(input.stores.keys())
            .filter(|d| stores.get(*d) != input.stores.get(*d))
            .cloned()
            .collect();
        changed.sort();
        changed.dedup();
        if !changed.is_empty() {
            let owner = owners(&per_unit);
            for d in &changed {
                match owner.get(d) {
                    Some(sis) => seeds.extend(sis.iter().copied()),
                    None => seeds.extend(0..solves.len()),
                }
            }
        }
        input.stores = stores;
        // Design §17.16 (b): the cells, from every unit's writes so far
        // (DECISIONS 197). The units reading a cell whose writes changed are
        // solved again.
        let mut cells: FxHashMap<String, Vec<CellWrite>> = FxHashMap::default();
        for (si, s) in solves.iter().enumerate() {
            let u = per_unit[si].as_ref().expect("every unit solved");
            for (k, c) in &u.cells_out {
                let v = cells.entry(k.clone()).or_default();
                let d = digest(c);
                let i = match v.iter().position(|w| w.digest == d && w.c == *c) {
                    Some(i) => i,
                    None => {
                        v.push(CellWrite::new(c.clone(), d));
                        v.len() - 1
                    }
                };
                v[i].writers.push(s.key.clone());
            }
        }
        for v in cells.values_mut() {
            for w in v.iter_mut() {
                w.writers.sort_unstable();
            }
            v.sort_by(|x, y| x.writers.cmp(&y.writers));
        }
        let cells_top = per_unit.iter().flatten().any(|u| u.stats.aborted);
        // A unit is solved again when what it READS of a cell changed: the
        // other writers' contracts (its own write is in its own solve,
        // DECISIONS 89), an unknown one or a unit over the bound being ⊤
        // that absorbs every other unknown. A reader forwarding the ⊤ it
        // was seeded with changes no other reader's view.
        // DECISIONS 169: the view is the writes' digests (taken once per
        // round, `CellWrite`), not their contracts cloned and sorted per
        // reader: AWFY rounds 2-4 viewed 180K cell reads, 18.8 s of 26.6.
        let view = |cells: &FxHashMap<String, Vec<CellWrite>>, top: bool, me: &str, k: &str| {
            let others: Vec<&CellWrite> = cells
                .get(k)
                .into_iter()
                .flatten()
                .filter(|w| !(w.writers.len() == 1 && w.writers[0] == me))
                .collect();
            let pure = |c: &ValueContract| {
                c.unknown
                    && c.layouts.is_empty()
                    && c.classes.is_empty()
                    && c.fns.is_empty()
                    && c.builtins.is_empty()
                    && c.arrays.is_empty()
                    && c.boxes.is_empty()
            };
            let top = top || others.iter().any(|w| pure(&w.c));
            let mut v: Vec<u64> = others
                .into_iter()
                .filter(|w| !pure(&w.c))
                .map(|w| if top { w.top } else { w.digest })
                .collect();
            v.sort_unstable();
            v.dedup();
            (top, v)
        };
        // A layout no unit published this round is gone; a unit that read
        // a layout which changed during the round read it before its
        // publisher solved, or the old one: solved again (DECISIONS 103).
        input.layouts.retain(|k, _| published_layouts.contains(k));
        copy_of.retain(|k, _| input.layouts.contains_key(k));
        owned_layouts.retain(|k| owned_now.contains(k));
        input.own_fns.retain(|k, _| published_fns.contains(k));
        // DECISIONS 114: each contract is digested once per round, as its
        // readers recorded it (`Gen::owner_fn`, `Gen::other_layout`): a
        // read compares two numbers, so the round does not grow with the
        // reads (awfy_Richards: 58544 reads of 39 layouts, each digest the
        // Debug text of a whole contract).
        let gone = digest(&None::<&ClassContract>);
        let fns_now: FxHashMap<&str, u64> = input
            .own_fns
            .iter()
            .map(|(k, e)| (k.as_str(), e.digest))
            .collect();
        let layouts_now: FxHashMap<&str, u64> = input
            .layouts
            .iter()
            .map(|(k, e)| (k.as_str(), e.digest))
            .collect();
        compares.set(compares.get() + (fns_now.len() + layouts_now.len()) as u64);
        let fn_changed =
            |(k, h): &(String, u64)| fns_now.get(k.as_str()).copied().unwrap_or(gone) != *h;
        for (si, u) in per_unit.iter().enumerate() {
            let u = u.as_ref().expect("every unit solved");
            // A unit over the bound keeps its result (`dependents`): what it
            // read changes nothing it publishes.
            if !u.stats.aborted && u.fns_read.iter().any(fn_changed) {
                seeds.push(si);
            }
        }
        let layout_changed =
            |(k, h): &(String, u64)| layouts_now.get(k.as_str()).copied().unwrap_or(gone) != *h;
        for (si, u) in per_unit.iter().enumerate() {
            let u = u.as_ref().expect("every unit solved");
            // A unit over the bound keeps its result (`dependents`): what it
            // read changes nothing it publishes.
            // DECISIONS 134: a unit's own forwarded copy is built from
            // what it read through the other channels, each tracked: it
            // does not solve its publisher again.
            let own_copy = |(k, _): &&(String, u64)| copy_of.get(k.as_str()) == Some(&si);
            if !u.stats.aborted
                && u.layouts_read
                    .iter()
                    .filter(|r| !own_copy(r))
                    .any(layout_changed)
            {
                seeds.push(si);
            }
        }
        // A cell whose writes and whose ⊤ are as last round reads alike in
        // every unit: only the changed ones are viewed per reader
        // (DECISIONS 114).
        let mut keys: Vec<&String> = cells.keys().chain(input.cells.keys()).collect();
        keys.sort_unstable();
        keys.dedup();
        compares.set(compares.get() + keys.len() as u64);
        let changed: FxHashSet<&str> = keys
            .into_iter()
            .filter(|k| input.cells.get(*k) != cells.get(*k))
            .map(|k| k.as_str())
            .collect();
        let top_changed = input.cells_top != cells_top;
        for (si, u) in per_unit.iter().enumerate() {
            let u = u.as_ref().expect("every unit solved");
            let me = solves[si].key.as_str();
            if u.cells_read.iter().any(|k| {
                (top_changed || changed.contains(k.as_str())) && {
                    compares.set(compares.get() + 1);
                    view(&input.cells, input.cells_top, me, k) != view(&cells, cells_top, me, k)
                }
            }) {
                seeds.push(si);
            }
        }
        drop(changed);
        input.cells = cells;
        input.cells_top = cells_top;
        // The write effects: last round's joined with what every unit's
        // latest solve stores into other units' bindings (an aborted unit:
        // any value into each it writes), so they only grow (D16, as the
        // argument contracts). The unit owning a binding whose writes grew
        // holds them, solved again with its dependents.
        let mut writes = input.writes.clone();
        for u in per_unit.iter().flatten() {
            for (k, vc) in &u.writes_out {
                writes.entry(k.clone()).or_default().join(vc);
            }
        }
        let mut grown: Vec<&String> = writes
            .iter()
            .filter(|(k, v)| input.writes.get(*k) != Some(*v))
            .map(|((p, _), _)| p)
            .collect();
        grown.sort();
        grown.dedup();
        for p in grown {
            seeds.extend(solves.iter().position(|s| &s.key == p));
        }
        input.writes = writes;
        // The shared objects' writes (`model::SharedWrite`): last round's
        // joined with every unit's latest, so they only grow (D16). When
        // they grew, the units that can see a new one are solved again.
        let mut shared = input.shared.clone();
        for u in per_unit.iter().flatten() {
            shared.extend(u.shared_out.iter().cloned());
        }
        shared_closure(&mut shared);
        if shared != input.shared {
            // A unit solved again only for a write it has not taken (one it
            // replayed, or one it performs itself with the replay's effect)
            // and can see (`model::SharedReads`).
            for (si, u) in per_unit.iter().enumerate() {
                let seen = &shared_rounds[shared_seen[si]];
                let u = u.as_ref().expect("every unit solved");
                if shared.iter().any(|w| {
                    !seen.contains(w)
                        && !(replay_is_own(w) && u.shared_out.contains(w))
                        && !u.stats.aborted
                        && u.shared_reads.sees(w)
                }) {
                    seeds.push(si);
                }
            }
            shared_rounds.push(shared.clone());
        }
        input.shared = shared;
        if cfg.report {
            eprintln!(
                "perry: wpfacts census r{rounds} end seeds {} {}",
                seeds.len(),
                census(&per_unit, &input, &shared_rounds)
            );
        }
        let fresh: Vec<String> = reports
            .into_iter()
            .filter(|d| !input.broken.contains(d))
            .collect();
        if !fresh.is_empty() || !seeds.is_empty() {
            dirty = if fresh.is_empty() {
                vec![false; solves.len()]
            } else {
                resolve_again(&solves, &per_unit, &fresh)
            };
            // DECISIONS 170: a seeded unit is solved again; its importers
            // follow when its outputs change (the level loop above). A
            // newly broken class still re-solves its owner's importers
            // (`resolve_again`): they read the broken set itself.
            for si in seeds {
                if per_unit[si].as_ref().is_some_and(|u| !u.stats.aborted) {
                    dirty[si] = true;
                }
            }
            input.broken.extend(fresh);
            continue;
        }
        let mut pf = ProgramFacts {
            module_paths: sorted.iter().map(|(p, _)| p.to_string()).collect(),
            dynamic_code_reach: cfg.dynamic_code_reach.clone(),
            sabotage: cfg.sabotage.clone(),
            bindings: vec![Default::default(); sorted.len()],
            indexed_metadata_unwritten: true,
            ..Default::default()
        };
        if cfg.unit_io {
            pf.unit_io = unit_io(sorted, &solves, &order, &per_unit, &input);
        }
        // The cells' writes at the fixpoint, each once per unit writing it
        // (each unit holds its own), for the tests to read their size.
        #[cfg(test)]
        for (k, ws) in &input.cells {
            for w in ws {
                pf.cells_out
                    .extend(w.writers.iter().map(|_| (k.clone(), w.c.clone())));
            }
        }
        for (si, u) in per_unit.into_iter().enumerate() {
            let u = u.expect("every unit solved");
            // Its layouts, for the tests to read the contracts' form
            // (DECISIONS 208).
            #[cfg(test)]
            pf.layouts_out.extend(u.layouts_out.iter().cloned());
            merge_unit(&mut pf, solves[si].mi, &solves[si].key, &solves[si].view, u);
        }
        for (path, b) in pf.module_paths.iter().zip(&mut pf.bindings) {
            unique_bindings(&mut b.locals, path, "LocalId");
            unique_bindings(&mut b.this_of, path, "this-of FuncId");
        }
        pf.stats.modules = sorted.len();
        typed_param_verdicts(&mut pf);
        pf.rounds = rounds;
        // The cost of the run is every round's.
        pf.stats.work = round_work.iter().sum();
        pf.round_work = round_work;
        pf.compares = compares.get();
        pf.broken = {
            let mut b: Vec<String> = input.broken.iter().cloned().collect();
            b.sort();
            b
        };
        // The program's writes of the shared objects: every unit's, joined.
        pf.shared_out = input.shared.iter().cloned().collect();
        return pf;
    }
}

/// The driver's held state for the census line (`FactsConfig::report`):
/// what the latest solves keep and what the round publishes, with the RSS.
fn census(
    per_unit: &[Option<ProgramFacts>],
    input: &Published,
    shared_rounds: &[std::collections::BTreeSet<crate::model::SharedWrite>],
) -> String {
    let us = per_unit.iter().flatten();
    let sum = |f: &dyn Fn(&ProgramFacts) -> usize| us.clone().map(f).sum::<usize>();
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let mib = |k: &str| {
        status
            .lines()
            .find(|l| l.starts_with(k))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
            >> 10
    };
    let handed = input
        .shared
        .iter()
        .filter(|w| matches!(w, crate::model::SharedWrite::Handed(_)))
        .count();
    let reads = |u: &ProgramFacts| {
        u.shared_reads.keys.len()
            + u.shared_reads
                .objects
                .values()
                .map(|k| k.as_ref().map_or(1, |k| k.len()))
                .sum::<usize>()
    };
    format!(
        "| rss {} hwm {} MiB | units {} sites {} tmpls {} toks {} shared_out {} cells_out {} cells_read {} layouts_out {} layouts_read {} fns_read {} contracts_out {} values_out {} shared_reads {} | in layouts {} classes {} fns {} values {} own_fns {} cells {} shared {} handed {} writes {} stores {} args {} | shared_rounds {} entries {}",
        mib("VmRSS:"),
        mib("VmHWM:"),
        us.clone().count(),
        sum(&|u| u.sites.len()),
        sum(&|u| u.templates.len()),
        sum(&|u| u.tok_names.len()),
        sum(&|u| u.shared_out.len()),
        sum(&|u| u.cells_out.len()),
        sum(&|u| u.cells_read.len()),
        sum(&|u| u.layouts_out.len()),
        sum(&|u| u.layouts_read.len()),
        sum(&|u| u.fns_read.len()),
        sum(&|u| u.contracts_out.len()),
        sum(&|u| u.values_out.len()),
        sum(&reads),
        input.layouts.len(),
        input.classes.len(),
        input.fns.len(),
        input.values.len(),
        input.own_fns.len(),
        input.cells.len(),
        input.shared.len(),
        handed,
        input.writes.len(),
        input.stores.values().map(|v| v.len()).sum::<usize>(),
        input.args.len(),
        shared_rounds.len(),
        shared_rounds.iter().map(|s| s.len()).sum::<usize>(),
    )
}

/// Does replaying `w` (`Model::replay_shared`) do exactly what the unit
/// performing `w` itself already did? A store or accessor on ⊤, an
/// unmodeled reach and a handover run the same rules either way; a store
/// or accessor on one object puts the other unit's unknown value there.
pub(super) fn replay_is_own(w: &crate::model::SharedWrite) -> bool {
    use crate::model::{Shared, SharedWrite};
    match w {
        SharedWrite::Store(s, _) | SharedWrite::Accessor(s, _) => *s == Shared::Top,
        SharedWrite::Unmodeled(_) | SharedWrite::Handed(_) | SharedWrite::Hard(..) => true,
    }
}

/// A runtime-owned object one unit hands to unknown code (`SharedWrite::Handed`)
/// is ⊤ in the other units' code: each store and accessor any unit performs on
/// ⊤ is one on that object, as the whole-program solve sees it on the
/// object's own token. A store under a computed key covers every key of its
/// object: the stores under one key beside it are dropped, so a write that
/// says nothing new solves no unit again.
pub(super) fn shared_closure(shared: &mut std::collections::BTreeSet<crate::model::SharedWrite>) {
    use crate::model::{Shared, SharedWrite};
    let handed: Vec<Shared> = shared
        .iter()
        .filter_map(|w| match w {
            SharedWrite::Handed(s) => Some(s.clone()),
            _ => None,
        })
        .collect();
    let on_top: Vec<SharedWrite> = shared
        .iter()
        .filter(|w| {
            matches!(
                w,
                SharedWrite::Store(Shared::Top, _) | SharedWrite::Accessor(Shared::Top, _)
            )
        })
        .cloned()
        .collect();
    for s in &handed {
        for w in &on_top {
            shared.insert(match w {
                SharedWrite::Store(_, k) => SharedWrite::Store(s.clone(), k.clone()),
                SharedWrite::Accessor(_, k) => SharedWrite::Accessor(s.clone(), k.clone()),
                _ => unreachable!("only stores and accessors on ⊤"),
            });
        }
    }
    let computed: Vec<Shared> = shared
        .iter()
        .filter_map(|w| match w {
            SharedWrite::Store(s, None) => Some(s.clone()),
            _ => None,
        })
        .collect();
    shared.retain(|w| !matches!(w, SharedWrite::Store(s, Some(_)) if computed.contains(s)));
}

/// Decision 32: the units by dependency level, each level in `order`
/// sequence. A unit is one level above the highest unit it is ordered
/// after; a back edge of a cycle (to a unit `order` puts later) is ⊤ for
/// it, as in the walk, and does not count. A unit reads other units only
/// through its `after` names (decision 21), all at lower levels, so a
/// level's units are independent of each other.
pub(super) fn dependency_levels(solves: &[Solve], order: &[usize]) -> Vec<Vec<usize>> {
    let mut pos = vec![0usize; solves.len()];
    for (p, &si) in order.iter().enumerate() {
        pos[si] = p;
    }
    let mut level = vec![0usize; solves.len()];
    let mut out: Vec<Vec<usize>> = Vec::new();
    for &si in order {
        let l = solves[si]
            .after
            .iter()
            .filter(|&&w| pos[w] < pos[si])
            .map(|&w| level[w] + 1)
            .max()
            .unwrap_or(0);
        level[si] = l;
        if out.len() <= l {
            out.resize_with(l + 1, Vec::new);
        }
        out[l].push(si);
    }
    out
}

/// Decision 32: one level's dirty units, solved against the one frozen
/// input on up to `jobs` threads. Results go back by unit, so the thread
/// count changes the wall and nothing else.
pub(super) fn solve_level(
    todo: &[usize],
    solves: &[Solve],
    sorted: &[(&str, &Module)],
    ucfg: &FactsConfig,
    input: &Published,
) -> Vec<(usize, ProgramFacts)> {
    let solve = |si: usize| {
        let s = &solves[si];
        let list = [(sorted[s.mi].0.to_string(), s.module)];
        (
            si,
            crate::compute_in(&list, ucfg, Some(input), Some(&s.view)),
        )
    };
    let jobs = ucfg.jobs.clamp(1, todo.len().max(1));
    if jobs == 1 {
        return todo.iter().map(|&si| solve(si)).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|sc| {
        let workers: Vec<_> = (0..jobs)
            .map(|_| {
                // The calling thread's recursion depth, not a spawned
                // thread's default 2 MiB.
                std::thread::Builder::new()
                    .stack_size(256 << 20)
                    .spawn_scoped(sc, || {
                        let mut mine = Vec::new();
                        while let Some(&si) =
                            todo.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
                        {
                            mine.push(solve(si));
                        }
                        mine
                    })
                    .expect("spawn a unit solver")
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().expect("a unit solve panicked"))
            .collect()
    })
}

/// `FactsConfig::unit_io` (design §13.3): per unit, a digest of every input
/// it reads (the contracts of the units it is ordered after, its closed
/// functions' argument contracts, the store effects on and the census
/// breaks of what it owns or reads) and of its facts, plus one line per
/// published output. Two dumps of an edited program name the units an edit
/// reaches and the contract that carried it.
pub(super) fn unit_io(
    sorted: &[(&str, &Module)],
    solves: &[Solve],
    order: &[usize],
    per_unit: &[Option<ProgramFacts>],
    input: &Published,
) -> Vec<String> {
    use crate::dump::{fnv, FNV_OFFSET};
    let mut pos = vec![0usize; solves.len()];
    for (p, &si) in order.iter().enumerate() {
        pos[si] = p;
    }
    let mut outs: Vec<Vec<(String, u64)>> = Vec::with_capacity(solves.len());
    let mut owned: Vec<Vec<String>> = Vec::with_capacity(solves.len());
    for u in per_unit {
        let u = u.as_ref().expect("every unit solved");
        let mut o: Vec<(String, u64)> = Vec::new();
        for ((_, n), c) in &u.contracts_out {
            o.push((format!("class:{n}"), digest(c)));
        }
        for ((_, n), c) in &u.fn_contracts_out {
            o.push((format!("fn:{n}"), digest(c)));
        }
        for ((_, n), c) in &u.values_out {
            o.push((format!("value:{n}"), digest(c)));
        }
        for c in &u.layouts_out {
            o.push((format!("layout:{}", c.display), digest(c)));
        }
        for (n, c) in &u.args_out {
            o.push((format!("args:{n}"), digest(c)));
        }
        for ((p, n), c) in &u.ctor_args_out {
            o.push((format!("args:{p}:{n}"), digest(c)));
        }
        for (n, c) in &u.this_args_out {
            o.push((format!("this:{n}"), digest(c)));
        }
        for n in &u.method_runs_out {
            o.push((format!("runs:{n}"), 0));
        }
        for (n, c) in &u.method_args_out {
            o.push((format!("margs:{n}"), digest(c)));
        }
        for (n, c) in &u.ctor_runs_out {
            o.push((format!("news:{n}"), digest(c)));
        }
        for (d, k, kinds) in &u.stores_out {
            o.push((format!("store:{d}.{k}"), digest(kinds)));
        }
        for (k, c) in &u.cells_out {
            o.push((format!("cell:{k}"), digest(c)));
        }
        for ((p, n), vc) in &u.writes_out {
            o.push((format!("write:{p}.{n}"), digest(vc)));
        }
        for d in &u.reports {
            o.push((format!("report:{d}"), 0));
        }
        o.sort();
        o.dedup();
        outs.push(o);
        let mut w: Vec<String> = u
            .templates
            .iter()
            .filter(|t| t.module == Some(0))
            .map(|t| t.display.clone())
            .collect();
        w.extend(u.fns_defined.iter().cloned());
        owned.push(w);
    }
    let mut lines = Vec::new();
    for (si, s) in solves.iter().enumerate() {
        let u = per_unit[si].as_ref().expect("every unit solved");
        let path = sorted[s.mi].0;
        let mut after: Vec<usize> = s
            .after
            .iter()
            .copied()
            .filter(|&w| pos[w] < pos[si])
            .collect();
        after.sort();
        after.dedup();
        let mut h = FNV_OFFSET;
        for &w in &after {
            fnv(&mut h, &solves[w].key);
            for (n, x) in &outs[w] {
                fnv(&mut h, &format!("{n}={x:016x}"));
            }
        }
        let mut closed: Vec<&String> = s.view.closed.keys().collect();
        closed.sort();
        let closed_names: Vec<&str> = closed.iter().map(|n| n.as_str()).collect();
        for nm in closed {
            fnv(
                &mut h,
                &format!(
                    "arg {nm} {:?}",
                    input.args.get(&(path.to_string(), nm.clone()))
                ),
            );
        }
        let mut seen: Vec<&String> = owned[si]
            .iter()
            .chain(after.iter().flat_map(|&w| owned[w].iter()))
            .collect();
        seen.sort();
        seen.dedup();
        for d in seen {
            fnv(
                &mut h,
                &format!(
                    "store {d} {:?} broken {}",
                    input.stores.get(d),
                    input.broken.contains(d)
                ),
            );
        }
        let mut f = FNV_OFFSET;
        for t in u.templates.iter().filter(|t| t.module == Some(0)) {
            for l in crate::dump::template_lines(t) {
                fnv(&mut f, &l);
            }
        }
        for st in &u.sites {
            fnv(&mut f, &format!("{st:?}"));
        }
        let st = &u.stats;
        // Its declarations by name (the first few): where an edit lands.
        let mut decls: Vec<String> = s
            .view
            .classes
            .iter()
            .map(|&k| format!("class:{}", s.module.classes[k].name))
            .collect();
        decls.extend(
            s.view
                .funcs
                .iter()
                .map(|&k| s.module.functions[k].name.clone()),
        );
        let more = decls.len().saturating_sub(6);
        decls.truncate(6);
        if more > 0 {
            decls.push(format!("+{more}"));
        }
        let keys = |v: &[usize]| {
            v.iter()
                .map(|&w| solves[w].key.as_str())
                .collect::<Vec<_>>()
                .join(",")
        };
        let mut deps = s.deps.clone();
        deps.sort();
        deps.dedup();
        lines.push(format!(
            "# uio {} work={} ms={:.1} in={h:016x} facts={f:016x} after={} deps={} closed={} decls={}",
            s.key,
            st.work,
            st.gen_ms + st.solve_ms + st.derive_ms,
            keys(&after),
            keys(&deps),
            closed_names.join(","),
            decls.join(",")
        ));
        for (n, x) in &outs[si] {
            lines.push(format!("# uout {} {n} {x:016x}", s.key));
        }
        // The cells it read (DECISIONS 171: the ones its solve consumed).
        let mut reads: Vec<&String> = u.cells_read.iter().collect();
        reads.sort();
        reads.dedup();
        for k in reads {
            lines.push(format!("# uread {} cell:{k}", s.key));
        }
    }
    lines
}

/// The units a newly broken contract changes (design §11.3): the unit
/// defining it (a template it owns, or a function it publishes) and every
/// unit depending on that one, transitively. A unit over the bound claims
/// and publishes nothing whatever its inputs, so it keeps its result and
/// passes no change on: solving it again would only pay the bound again. A
/// display no unit defines makes every unit dirty.
pub(super) fn resolve_again(
    solves: &[Solve],
    per_unit: &[Option<ProgramFacts>],
    fresh: &[String],
) -> Vec<bool> {
    let n = solves.len();
    let owner = owners(per_unit);
    let mut work: Vec<usize> = Vec::new();
    for d in fresh {
        // An override report changes the overridden class's unit
        // (DECISIONS 59).
        // So does a typed-array field report (DECISIONS 126).
        let d = d
            .strip_prefix("override ")
            .or_else(|| d.strip_prefix("ta "))
            .and_then(|r| r.split_once(" of "))
            .map_or(d.as_str(), |(_, c)| c);
        match owner.get(d) {
            Some(sis) => work.extend(sis.iter().copied()),
            // No unit owns it: every unit again, but a unit over the bound
            // keeps its result, as in `dependents` (it claims nothing
            // whatever its inputs; tsc: the 200M-work unit was solved again
            // for one unowned display).
            None => {
                return (0..n)
                    .map(|si| !per_unit[si].as_ref().is_some_and(|u| u.stats.aborted))
                    .collect()
            }
        }
    }
    dependents(solves, per_unit, work)
}

/// A published contract and its digest (DECISIONS 114), taken once when
/// it is published: its readers record the number (`Published::layout_seen`)
/// and the round compares numbers, not contracts.
#[derive(Clone, Debug)]
pub(crate) struct Digested<T> {
    pub c: T,
    pub digest: u64,
}

impl<T: std::hash::Hash> Digested<T> {
    pub(crate) fn new(c: T) -> Self {
        let digest = digest(&Some(&c));
        Digested { c, digest }
    }
}

/// One distinct write of a cell (design §17.16 (b)): its contract and the
/// units writing it (their keys, sorted). DECISIONS 169: its digests are
/// taken once, when the round groups the writes (`digest`; `top`: with
/// the unknown value and the primitives dropped, as a reader whose view is
/// ⊤ sees it), so a reader's view of the cell compares numbers, as a
/// layout read does (DECISIONS 166).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CellWrite {
    pub digest: u64,
    pub top: u64,
    pub writers: Vec<String>,
    pub c: ValueContract,
}

impl CellWrite {
    pub(super) fn new(c: ValueContract, digest: u64) -> Self {
        let mut t = c.clone();
        t.unknown = false;
        t.prim = 0;
        CellWrite {
            digest,
            top: self::digest(&t),
            writers: Vec::new(),
            c,
        }
    }
}

/// What a reader saw of an entry read by name (a layout, a function
/// contract, DECISIONS 103/106): it is solved again when the entry differs.
pub(crate) fn digest<T: std::hash::Hash>(v: &T) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// The units defining each display. A display may have several owners:
/// every unit holding a copy of one literal shape (`decl_units`).
pub(super) fn owners(per_unit: &[Option<ProgramFacts>]) -> FxHashMap<String, Vec<usize>> {
    let mut owner: FxHashMap<String, Vec<usize>> = FxHashMap::default();
    for (si, u) in per_unit.iter().enumerate() {
        let Some(u) = u else { continue };
        for t in u.templates.iter().filter(|t| t.module == Some(0)) {
            owner.entry(t.display.clone()).or_default().push(si);
        }
        for d in &u.fns_defined {
            owner.entry(d.clone()).or_default().push(si);
        }
    }
    owner
}

/// Units `work` and every unit depending on one of them, transitively;
/// a unit over the bound keeps its result (`resolve_again`).
pub(super) fn dependents(
    solves: &[Solve],
    per_unit: &[Option<ProgramFacts>],
    mut work: Vec<usize>,
) -> Vec<bool> {
    let n = solves.len();
    let mut importers: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (si, s) in solves.iter().enumerate() {
        for &d in &s.deps {
            importers[d].push(si);
        }
    }
    let mut dirty = vec![false; n];
    while let Some(si) = work.pop() {
        if per_unit[si].as_ref().is_some_and(|u| u.stats.aborted) {
            continue;
        }
        if !std::mem::replace(&mut dirty[si], true) {
            work.extend(importers[si].iter().copied());
        }
    }
    dirty
}

/// A unit's bodies by program module: its solve's module 0 is the unit's
/// own module (`mi`); a body of any other module is no claim here.
pub(super) fn program_bodies(bodies: &mut Vec<crate::bindings::BodyRef>, mi: usize) {
    if bodies.iter().any(|b| b.module != 0) {
        bodies.clear();
    }
    for b in bodies.iter_mut() {
        b.module = mi as u32;
    }
}

pub(super) fn merge_unit(
    pf: &mut ProgramFacts,
    mi: usize,
    path: &str,
    view: &crate::decl_units::UnitView,
    u: ProgramFacts,
) {
    for mut t in u.templates {
        if t.module != Some(0) {
            continue;
        }
        t.module = Some(mi as u32);
        for f in t.fields.iter_mut() {
            program_bodies(&mut f.fns, mi);
        }
        t.id = pf.templates.len() as crate::model::TmplId;
        pf.templates.push(t);
    }
    pf.sites.extend(u.sites.into_iter().map(|s| SiteFacts {
        module: mi as u32,
        ..s
    }));
    if let Some(mut b) = u.bindings.into_iter().next() {
        for (_, f) in b.locals.iter_mut().chain(b.this_of.iter_mut()) {
            program_bodies(&mut f.bodies, mi);
        }
        let into = &mut pf.bindings[mi];
        into.locals.extend(
            b.locals
                .into_iter()
                .filter(|(id, _)| view.binding_locals.contains(id)),
        );
        into.numeric_functions.extend(
            b.numeric_functions
                .into_iter()
                .filter(|(id, _, _)| view.binding_funcs.contains(id)),
        );
        into.this_of.extend(
            b.this_of
                .into_iter()
                .filter(|(id, _)| view.binding_funcs.contains(id)),
        );
    }
    pf.indexed_metadata_unwritten &= u.indexed_metadata_unwritten;
    pf.super_unrelinked.extend(u.super_unrelinked);
    pf.interp_guard = pf.interp_guard.take().or(u.interp_guard);
    pf.fnproto_bind_written = pf.fnproto_bind_written.take().or(u.fnproto_bind_written);
    let (a, s) = (&mut pf.stats, &u.stats);
    a.templates += s.templates;
    a.fact_units += s.fact_units;
    a.nodes += s.nodes;
    a.constraints += s.constraints;
    a.tokens += s.tokens;
    a.functions += s.functions;
    a.gen_ms += s.gen_ms;
    a.solve_ms += s.solve_ms;
    a.derive_ms += s.derive_ms;
    a.pops += s.pops;
    a.instances += s.instances;
    a.shared_instances += s.shared_instances;
    a.composed += s.composed;
    a.contexts += s.contexts;
    a.work += s.work;
    a.top_calls += s.top_calls;
    a.pts_entries += s.pts_entries;
    pf.units.push((path.to_string(), u.stats));
}

/// Post-census verdicts for sites on a parameter whose TS type is an
/// object type (design §11.4): the type picks the candidates, every class
/// template of the program whose definite data fields carry the type's
/// keys; the entry guard is `class_id(param) ∈ candidates`. The site is E0
/// when every candidate is prefix-stable and holds the key in one slot.
pub(super) fn typed_param_verdicts(pf: &mut ProgramFacts) {
    let mut cand_of: FxHashMap<Vec<String>, Vec<usize>> = FxHashMap::default();
    for s in pf.sites.iter_mut() {
        let Some(keys) = s.guard_keys.take() else {
            continue;
        };
        if !matches!(s.verdict, crate::Verdict::No(_)) || s.is_call {
            continue;
        }
        let tfs = &pf.templates;
        let cands = cand_of.entry(keys.clone()).or_insert_with(|| {
            (0..tfs.len())
                .filter(|&i| {
                    let t = &tfs[i];
                    t.class_id.is_some()
                        && keys.iter().all(|k| {
                            t.fields
                                .iter()
                                .any(|f| &f.key == k && f.definite && f.data_field)
                        })
                })
                .collect()
        });
        if cands.is_empty() {
            continue;
        }
        let slot = |t: &TemplateFacts| {
            t.fields
                .iter()
                .find(|f| f.key == s.key && f.definite && f.data_field)
                .and_then(|f| f.slot)
        };
        let first = slot(&tfs[cands[0]]);
        if first.is_some()
            && cands
                .iter()
                .all(|&i| tfs[i].prefix_stable && slot(&tfs[i]) == first)
        {
            s.receivers = cands.iter().map(|&i| tfs[i].display.clone()).collect();
            s.verdict = crate::Verdict::E0;
        }
    }
}

/// Facts must name one lexical binding. A second declaration with the same
/// id is a compiler bug, not a choice of which declaration's proof to keep.
pub(super) fn unique_bindings<T>(values: &mut [(u32, T)], module: &str, kind: &str) {
    values.sort_by_key(|(id, _)| *id);
    for pair in values.windows(2) {
        assert_ne!(
            pair[0].0, pair[1].0,
            "per-unit facts: duplicate {kind} {} in {module}; cloned bodies must have distinct binding identities",
            pair[0].0,
        );
    }
}
