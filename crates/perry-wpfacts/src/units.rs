//! Per-unit driver pieces (decision 151, design §11): what a unit
//! publishes to the other units of the program.

use crate::pts::{FxHashMap, FxHashSet};
use crate::{FactsConfig, ProgramFacts, SiteFacts, TemplateFacts};
use perry_hir::{Export, Import, ImportSpecifier, Module};

/// A published class as its importers see it (design §11.2): its template
/// and the facts its defining unit proved, its prototype methods (key,
/// function name) along the chain inside that unit, and its native parent.
#[derive(Clone, Debug)]
pub(crate) struct ClassContract {
    pub display: String,
    pub name: String,
    pub class_id: u32,
    pub literal_shape: bool,
    pub facts: TemplateFacts,
    pub methods: Vec<(String, String)>,
    pub native_parent: Option<(String, String)>,
    /// Fields that hold exactly these functions of the defining unit, each
    /// a function contract (a literal's methods: `errorUtil.errToObj`);
    /// every other field holds unknown values for the reader.
    pub field_fns: Vec<(String, Vec<FnContract>)>,
    /// The defining unit stores under computed keys: a reader may find any
    /// key (unknown values). Otherwise the keys are the facts' fields.
    pub open_keys: bool,
    /// The class object's own members along the chain inside the defining
    /// unit (key, the function when it is a static method; else an unknown
    /// value), and whether it may hold other keys (computed static stores,
    /// a native parent): then every static is unknown.
    pub statics: Vec<(String, Option<String>)>,
    pub statics_open: bool,
    /// The static methods with a function contract (key, contract): the
    /// defining unit checked their results (an instance of a contract
    /// class is given out, `Model::give_out`).
    pub static_fns: Vec<(String, FnContract)>,
}

/// A published function as its callers see it (design §11.2): per
/// parameter what the callee does with everything reachable from the
/// argument (`model::MARK_*`: read, touched by a protocol, or used beyond:
/// then it escapes), whether extra arguments reach it (`variadic`: all
/// arguments escape), and the layouts it returns (`None`: ⊤).
#[derive(Clone, Debug)]
pub(crate) struct FnContract {
    pub name: String,
    /// Per parameter: what the callee does to the argument itself
    /// (`model::MARK_*`), and to what the argument reaches (its fields,
    /// transitively). Two sets: reading `p.n` and converting it is no
    /// conversion of `p`.
    pub params: Vec<u8>,
    pub reach: Vec<u8>,
    pub variadic: bool,
    pub ret: Option<Vec<String>>,
    /// The parameters whose argument the result may be, and those whose
    /// reachable values (fields, transitively) it may hold: result
    /// transfers, the caller's own objects come back, nothing escapes.
    pub ret_params: Vec<u32>,
    pub ret_reach: Vec<u32>,
    /// The result may also be an unknown value (⊤ to the caller).
    pub ret_unknown: bool,
    /// An arrow function: its receiver is not used.
    pub lexical_this: bool,
    /// The binding name of a closed function (decision 19): a unit that
    /// calls it as a value publishes what it passes under this name.
    pub closed: Option<String>,
}

/// A published module-scope value binding as its readers see it (design
/// §11.2 "an object contract", generalized to every binding another
/// declaration unit reads, §12): the layouts it may hold (literal-shape
/// objects of the defining unit, instances of contract classes, layouts it
/// holds through other contracts), the contract classes and functions it
/// may hold, and whether it may hold an unknown value (⊤ to the reader).
/// The layouts' fields hold unknown values for the reader; a store or an
/// opening operation through them is the reader's census report.
#[derive(Clone, Debug, Default)]
pub(crate) struct ValueContract {
    pub layouts: Vec<ClassContract>,
    pub classes: Vec<ClassContract>,
    pub fns: Vec<FnContract>,
    pub unknown: bool,
    /// The primitive kinds it may hold (`model::prim` bits).
    pub prim: u16,
}

/// What the calls of a closed function pass (decision 13,
/// `decl_units::closed_bindings`): per parameter the value contract of the
/// arguments, and whether some call may pass anything (`top`: a unit calls
/// it through ⊤ or a broken contract, or claims nothing over the bound).
/// One unit publishes its calls' (`ProgramFacts::args_out`); the callee
/// takes the union over every unit.
#[derive(Clone, Debug, Default)]
pub(crate) struct ArgContract {
    pub params: Vec<ValueContract>,
    pub top: bool,
}

impl ArgContract {
    fn join(&mut self, o: &ArgContract) {
        self.top |= o.top;
        if self.params.len() < o.params.len() {
            self.params.resize(o.params.len(), ValueContract::default());
        }
        for (a, b) in self.params.iter_mut().zip(&o.params) {
            for l in &b.layouts {
                if !a.layouts.iter().any(|x| x.display == l.display) {
                    a.layouts.push(l.clone());
                }
            }
            for c in &b.classes {
                if !a.classes.iter().any(|x| x.display == c.display) {
                    a.classes.push(c.clone());
                }
            }
            for f in &b.fns {
                if !a.fns.iter().any(|x| x.name == f.name) {
                    a.fns.push(f.clone());
                }
            }
            a.unknown |= b.unknown;
            a.prim |= b.prim;
            a.layouts.sort_by(|x, y| x.display.cmp(&y.display));
            a.classes.sort_by(|x, y| x.display.cmp(&y.display));
            a.fns.sort_by(|x, y| x.name.cmp(&y.name));
        }
    }
}

/// What the other units can name of each module (§11.1): the export names
/// some module imports or re-exports by name, or the whole namespace where
/// a module imports it as a namespace, re-exports all of it, or a dynamic
/// `import()` can reach it. Syntactic and linear in the import edges.
#[derive(Clone, Debug, Default)]
pub(crate) struct Published {
    whole: FxHashSet<String>,
    names: FxHashMap<String, FxHashSet<String>>,
    /// Interpreted code can import any module: everything is published.
    all: bool,
    /// A direct eval can name a module's bindings: its code may call any
    /// function of that module (no function is closed, decision 13).
    eval_scopes: bool,
    /// Templates (by display) the census found used beyond their contract:
    /// published as escapes.
    pub(crate) broken: FxHashSet<String>,
    /// The class contracts of the units solved so far, by (defining module
    /// path, export name).
    pub(crate) classes: FxHashMap<(String, String), ClassContract>,
    /// Their function contracts, by (defining module path, export name).
    pub(crate) fns: FxHashMap<(String, String), FnContract>,
    /// Their value contracts, by (defining module path, export name).
    pub(crate) values: FxHashMap<(String, String), ValueContract>,
    /// The layouts contract functions return, by template display.
    pub(crate) layouts: FxHashMap<String, ClassContract>,
    /// The argument contracts of closed functions (decision 13), by
    /// (module path, name): the union over every unit's calls.
    pub(crate) args: FxHashMap<(String, String), ArgContract>,
    /// Store effects (decision 17): per literal layout display, the
    /// existing keys other units store into (union over every unit).
    pub(crate) stores: FxHashMap<String, Vec<String>>,
}

impl Published {
    pub(crate) fn whole(&self, path: &str) -> bool {
        self.all || self.whole.contains(path)
    }

    pub(crate) fn fn_contract(&self, path: &str, name: &str) -> Option<&FnContract> {
        self.fns.get(&(path.to_string(), name.to_string()))
    }

    pub(crate) fn value_contract(&self, path: &str, name: &str) -> Option<&ValueContract> {
        self.values.get(&(path.to_string(), name.to_string()))
    }

    pub(crate) fn layout(&self, display: &str) -> Option<&ClassContract> {
        self.layouts.get(display)
    }

    pub(crate) fn class_contract(&self, path: &str, name: &str) -> Option<&ClassContract> {
        self.classes.get(&(path.to_string(), name.to_string()))
    }

    pub(crate) fn arg_contract(&self, path: &str, name: &str) -> Option<&ArgContract> {
        self.args.get(&(path.to_string(), name.to_string()))
    }

    /// A direct eval may name module bindings: interpreted code (which
    /// otherwise reaches only exports and globals) may call a module's
    /// functions by name. The reach does not say which module's, so none
    /// is closed.
    pub(crate) fn stored_keys(&self, display: &str) -> &[String] {
        self.stores.get(display).map_or(&[], |v| v.as_slice())
    }

    pub(crate) fn eval_scopes(&self) -> bool {
        self.eval_scopes
    }

    pub(crate) fn broken(&self, display: &str) -> bool {
        self.broken.contains(display)
    }

    pub(crate) fn names(&self, path: &str, name: &str) -> bool {
        self.whole(path) || self.names.get(path).is_some_and(|n| n.contains(name))
    }
}

pub(crate) fn published_surface(
    sorted: &[(&str, Vec<&Import>, &[Export])],
    cfg: &FactsConfig,
) -> Published {
    let mut p = Published {
        all: cfg.dynamic_code.is_some()
            || cfg.dynamic_code_reach.globals
            || !cfg.dynamic_code_reach.sites.is_empty(),
        eval_scopes: !cfg.dynamic_code_reach.lexical_modules.is_empty(),
        ..Default::default()
    };
    for (path, imports, exports) in sorted {
        let resolve = |source: &str| {
            imports
                .iter()
                .find(|i| i.source == source)
                .and_then(|i| i.resolved_path.clone())
        };
        for imp in imports.iter().copied() {
            let Some(t) = imp.resolved_path.as_ref().filter(|t| t.as_str() != *path) else {
                continue;
            };
            // Another declaration unit of the same module: its names are
            // the module's (§12.3).
            let t = &crate::decl_units::module_of(t).to_string();
            if imp.type_only || imp.runtime_erased {
                continue;
            }
            if imp.is_dynamic {
                p.whole.insert(t.clone());
            }
            for s in &imp.specifiers {
                match s {
                    ImportSpecifier::Named { imported, .. } => {
                        p.names.entry(t.clone()).or_default().insert(imported.clone());
                    }
                    ImportSpecifier::Default { .. } => {
                        p.names.entry(t.clone()).or_default().insert("default".into());
                    }
                    ImportSpecifier::Namespace { .. } => {
                        p.whole.insert(t.clone());
                    }
                }
            }
        }
        for e in exports.iter() {
            match e {
                Export::ReExport { source, imported, .. } => {
                    if let Some(t) = resolve(source) {
                        p.names.entry(t).or_default().insert(imported.clone());
                    }
                }
                Export::ExportAll { source } | Export::NamespaceReExport { source, .. } => {
                    if let Some(t) = resolve(source) {
                        p.whole.insert(t);
                    }
                }
                Export::Named { .. } => {}
            }
        }
    }
    p
}

/// One solve of the per-unit driver: a declaration unit of module `mi`
/// (design §12.6), its view, the solves it depends on (the units of its
/// module it names, every unit of each module it imports: what a census
/// round solves again), and the solves it is ordered after.
struct Solve<'a> {
    mi: usize,
    key: String,
    /// The one module the unit is cut from (its run-once view, N5, when
    /// it has one): borrowed, never copied per unit.
    module: &'a Module,
    view: crate::decl_units::UnitView,
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
fn owner_of(out: &[Solve], t: &std::ops::Range<usize>, name: &str) -> Option<usize> {
    t.clone().find(|&si| {
        out[si].view.exports.iter().any(|e| match e {
            Export::Named { exported, .. } | Export::ReExport { exported, .. } => exported == name,
            Export::NamespaceReExport { name: n, .. } => n == name,
            _ => false,
        })
    })
}

fn solves<'a>(sorted: &[(&'a str, &'a Module)], flats: &'a [Option<Module>]) -> Vec<Solve<'a>> {
    let index: FxHashMap<&str, usize> = sorted.iter().enumerate().map(|(i, (p, _))| (*p, i)).collect();
    let mut out: Vec<Solve<'a>> = Vec::new();
    let mut range: Vec<std::ops::Range<usize>> = Vec::with_capacity(sorted.len());
    for (mi, (path, module)) in sorted.iter().enumerate() {
        let base = out.len();
        // A body that runs once at a module-scope statement is module scope
        // for the cut (N5, §12.5): the module's run-once view, one per
        // module, shared by all its units.
        let module: &'a Module = flats[mi].as_ref().unwrap_or(module);
        for (k, (view, deps)) in crate::decl_units::views(path, module).into_iter().enumerate() {
            let deps: Vec<usize> = deps.into_iter().map(|d| base + d).collect();
            out.push(Solve {
                mi,
                key: crate::decl_units::unit_path(path, k),
                module,
                view,
                after: deps.clone(),
                deps,
            });
        }
        range.push(base..out.len());
    }
    for si in 0..out.len() {
        let (path, module) = sorted[out[si].mi];
        let mut after: Vec<usize> = Vec::new();
        for imp in &module.imports {
            let Some(&t) = imp.resolved_path.as_deref().filter(|t| *t != path).and_then(|t| index.get(t)) else {
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
                match name.and_then(|n| owner_of(&out, &range[t], n)) {
                    Some(w) => after.push(w),
                    None => after.extend(range[t].clone()),
                }
            }
        }
        out[si].after.extend(after);
    }
    out
}

/// Solves in dependency order (a solve after the ones it depends on), so a
/// contract is computed before a unit consumes it. In an import cycle the
/// back edge has no contract yet: that import is ⊤, as in P1.
fn dependency_order(solves: &[Solve]) -> Vec<usize> {
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
/// every other unit's inputs are unchanged, so its result is kept. Broken
/// only grows, so this ends.

pub(crate) fn run(sorted: &[(&str, &Module)], cfg: &FactsConfig) -> ProgramFacts {
    let ucfg = FactsConfig { unit: true, ..cfg.clone() };
    let flats: Vec<Option<Module>> = sorted.iter().map(|(_, m)| crate::decl_units::run_once_flat(m)).collect();
    let solves = solves(sorted, &flats);
    let viewed: Vec<(&str, Vec<&Import>, &[Export])> = solves
        .iter()
        .map(|s| (sorted[s.mi].0, s.module.imports.iter().chain(s.view.imports.iter()).collect(), s.view.exports.as_slice()))
        .collect();
    let mut input = published_surface(&viewed, cfg);
    let order = dependency_order(&solves);
    let levels = dependency_levels(&solves, &order);
    let mut rounds = 0u32;
    let mut round_work: Vec<u64> = Vec::new();
    let mut per_unit: Vec<Option<ProgramFacts>> = (0..solves.len()).map(|_| None).collect();
    let mut dirty = vec![true; solves.len()];
    loop {
        rounds += 1;
        input.classes.clear();
        input.fns.clear();
        input.values.clear();
        input.layouts.clear();
        let mut reports: FxHashSet<String> = FxHashSet::default();
        let mut work = 0u64;
        for level in &levels {
            let todo: Vec<usize> = level.iter().copied().filter(|&si| dirty[si]).collect();
            for (si, u) in solve_level(&todo, &solves, sorted, &ucfg, &input) {
                work += u.stats.work;
                per_unit[si] = Some(u);
            }
            // The level's contracts, published in `order` sequence: the
            // next level's one input.
            for &si in level {
                let s = &solves[si];
                let u = per_unit[si].as_ref().expect("every unit solved");
                // Published under the module's path (other modules import it)
                // and the unit's (the module's other units import it).
                for ((p, n), c) in &u.contracts_out {
                    input.classes.insert((s.key.clone(), n.clone()), c.clone());
                    input.classes.insert((p.clone(), n.clone()), c.clone());
                }
                for ((p, n), c) in &u.fn_contracts_out {
                    input.fns.insert((s.key.clone(), n.clone()), c.clone());
                    input.fns.insert((p.clone(), n.clone()), c.clone());
                }
                for ((p, n), c) in &u.values_out {
                    input.values.insert((s.key.clone(), n.clone()), c.clone());
                    input.values.insert((p.clone(), n.clone()), c.clone());
                }
                for c in &u.layouts_out {
                    input.layouts.insert(c.display.clone(), c.clone());
                }
                reports.extend(u.reports.iter().cloned());
            }
        }
        round_work.push(work);
        // Decision 13: the argument contracts of closed functions, from
        // every unit's latest solve. A closed function whose contract
        // changed is solved again, with the units depending on it; at the
        // fixpoint every closed function's parameters hold what its calls
        // pass.
        let mut args: FxHashMap<(String, String), ArgContract> = FxHashMap::default();
        for (si, s) in solves.iter().enumerate() {
            let path = sorted[s.mi].0;
            let u = per_unit[si].as_ref().expect("every unit solved");
            if u.stats.aborted {
                for nm in &s.view.closed_refs {
                    args.entry((path.to_string(), nm.clone())).or_default().top = true;
                }
                continue;
            }
            for (nm, ac) in &u.args_out {
                args.entry((path.to_string(), nm.clone())).or_default().join(ac);
            }
        }
        let mut seeds: Vec<usize> = Vec::new();
        for (si, s) in solves.iter().enumerate() {
            let path = sorted[s.mi].0;
            let changed = s.view.closed.keys().any(|nm| {
                let k = (path.to_string(), nm.clone());
                format!("{:?}", args.get(&k)) != format!("{:?}", input.args.get(&k))
            });
            if changed {
                seeds.push(si);
            }
        }
        input.args = args;
        // Decision 17: the store effects, from every unit's latest solve.
        // The owners of a layout whose effect keys changed perform them,
        // solved again with their dependents.
        let mut stores: FxHashMap<String, Vec<String>> = FxHashMap::default();
        for u in per_unit.iter().flatten() {
            for (d, k) in &u.stores_out {
                let v = stores.entry(d.clone()).or_default();
                if !v.contains(k) {
                    v.push(k.clone());
                }
            }
        }
        for v in stores.values_mut() {
            v.sort();
        }
        let changed: Vec<String> = stores
            .iter()
            .filter(|(d, v)| input.stores.get(*d) != Some(*v))
            .map(|(d, _)| d.clone())
            .collect();
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
        let fresh: Vec<String> = reports.into_iter().filter(|d| !input.broken.contains(d)).collect();
        if !fresh.is_empty() || !seeds.is_empty() {
            dirty = if fresh.is_empty() { vec![false; solves.len()] } else { resolve_again(&solves, &per_unit, &fresh) };
            for (si, d) in dependents(&solves, &per_unit, seeds).into_iter().enumerate() {
                dirty[si] |= d;
            }
            input.broken.extend(fresh);
            continue;
        }
        let mut pf = ProgramFacts {
            module_paths: sorted.iter().map(|(p, _)| p.to_string()).collect(),
            dynamic_code_reach: cfg.dynamic_code_reach.clone(),
            sabotage: cfg.sabotage.clone(),
            bindings: vec![Default::default(); sorted.len()],
            ..Default::default()
        };
        if cfg.unit_io {
            pf.unit_io = unit_io(sorted, &solves, &order, &per_unit, &input);
        }
        for (si, u) in per_unit.into_iter().enumerate() {
            let u = u.expect("every unit solved");
            merge_unit(&mut pf, solves[si].mi, &solves[si].key, u);
        }
        for b in pf.bindings.iter_mut() {
            b.locals.sort_by_key(|(id, _)| *id);
            b.locals.dedup_by_key(|(id, _)| *id);
            b.this_of.sort_by_key(|(id, _)| *id);
            b.this_of.dedup_by_key(|(id, _)| *id);
        }
        pf.stats.modules = sorted.len();
        typed_param_verdicts(&mut pf);
        pf.rounds = rounds;
        // The cost of the run is every round's.
        pf.stats.work = round_work.iter().sum();
        pf.round_work = round_work;
        pf.broken = {
            let mut b: Vec<String> = input.broken.iter().cloned().collect();
            b.sort();
            b
        };
        return pf;
    }
}

/// Decision 32: the units by dependency level, each level in `order`
/// sequence. A unit is one level above the highest unit it is ordered
/// after; a back edge of a cycle (to a unit `order` puts later) is ⊤ for
/// it, as in the walk, and does not count. A unit reads other units only
/// through its `after` names (decision 21), all at lower levels, so a
/// level's units are independent of each other.
fn dependency_levels(solves: &[Solve], order: &[usize]) -> Vec<Vec<usize>> {
    let mut pos = vec![0usize; solves.len()];
    for (p, &si) in order.iter().enumerate() {
        pos[si] = p;
    }
    let mut level = vec![0usize; solves.len()];
    let mut out: Vec<Vec<usize>> = Vec::new();
    for &si in order {
        let l = solves[si].after.iter().filter(|&&w| pos[w] < pos[si]).map(|&w| level[w] + 1).max().unwrap_or(0);
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
fn solve_level(
    todo: &[usize],
    solves: &[Solve],
    sorted: &[(&str, &Module)],
    ucfg: &FactsConfig,
    input: &Published,
) -> Vec<(usize, ProgramFacts)> {
    let solve = |si: usize| {
        let s = &solves[si];
        let list = [(sorted[s.mi].0.to_string(), s.module)];
        (si, crate::compute_in(&list, ucfg, Some(input), Some(&s.view)))
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
                        while let Some(&si) = todo.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) {
                            mine.push(solve(si));
                        }
                        mine
                    })
                    .expect("spawn a unit solver")
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().expect("a unit solve panicked")).collect()
    })
}

/// `FactsConfig::unit_io` (design §13.3): per unit, a digest of every input
/// it reads (the contracts of the units it is ordered after, its closed
/// functions' argument contracts, the store effects on and the census
/// breaks of what it owns or reads) and of its facts, plus one line per
/// published output. Two dumps of an edited program name the units an edit
/// reaches and the contract that carried it.
fn unit_io(
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
    let hash = |s: &str| {
        let mut h = FNV_OFFSET;
        fnv(&mut h, s);
        h
    };
    let mut outs: Vec<Vec<(String, u64)>> = Vec::with_capacity(solves.len());
    let mut owned: Vec<Vec<String>> = Vec::with_capacity(solves.len());
    for u in per_unit {
        let u = u.as_ref().expect("every unit solved");
        let mut o: Vec<(String, u64)> = Vec::new();
        for ((_, n), c) in &u.contracts_out {
            o.push((format!("class:{n}"), hash(&format!("{c:?}"))));
        }
        for ((_, n), c) in &u.fn_contracts_out {
            o.push((format!("fn:{n}"), hash(&format!("{c:?}"))));
        }
        for ((_, n), c) in &u.values_out {
            o.push((format!("value:{n}"), hash(&format!("{c:?}"))));
        }
        for c in &u.layouts_out {
            o.push((format!("layout:{}", c.display), hash(&format!("{c:?}"))));
        }
        for (n, c) in &u.args_out {
            o.push((format!("args:{n}"), hash(&format!("{c:?}"))));
        }
        for (d, k) in &u.stores_out {
            o.push((format!("store:{d}.{k}"), 0));
        }
        for d in &u.reports {
            o.push((format!("report:{d}"), 0));
        }
        o.sort();
        o.dedup();
        outs.push(o);
        let mut w: Vec<String> = u.templates.iter().filter(|t| t.module == Some(0)).map(|t| t.display.clone()).collect();
        w.extend(u.fns_defined.iter().cloned());
        owned.push(w);
    }
    let mut lines = Vec::new();
    for (si, s) in solves.iter().enumerate() {
        let u = per_unit[si].as_ref().expect("every unit solved");
        let path = sorted[s.mi].0;
        let mut after: Vec<usize> = s.after.iter().copied().filter(|&w| pos[w] < pos[si]).collect();
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
            fnv(&mut h, &format!("arg {nm} {:?}", input.args.get(&(path.to_string(), nm.clone()))));
        }
        let mut seen: Vec<&String> = owned[si].iter().chain(after.iter().flat_map(|&w| owned[w].iter())).collect();
        seen.sort();
        seen.dedup();
        for d in seen {
            fnv(&mut h, &format!("store {d} {:?} broken {}", input.stores.get(d), input.broken.contains(d)));
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
        let mut decls: Vec<String> = s.view.classes.iter().map(|&k| format!("class:{}", s.module.classes[k].name)).collect();
        decls.extend(s.view.funcs.iter().map(|&k| s.module.functions[k].name.clone()));
        let more = decls.len().saturating_sub(6);
        decls.truncate(6);
        if more > 0 {
            decls.push(format!("+{more}"));
        }
        let keys = |v: &[usize]| v.iter().map(|&w| solves[w].key.as_str()).collect::<Vec<_>>().join(",");
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
    }
    lines
}

/// The units a newly broken contract changes (design §11.3): the unit
/// defining it (a template it owns, or a function it publishes) and every
/// unit depending on that one, transitively. A unit over the bound claims
/// and publishes nothing whatever its inputs, so it keeps its result and
/// passes no change on: solving it again would only pay the bound again. A
/// display no unit defines makes every unit dirty.
fn resolve_again(solves: &[Solve], per_unit: &[Option<ProgramFacts>], fresh: &[String]) -> Vec<bool> {
    let n = solves.len();
    let owner = owners(per_unit);
    let mut work: Vec<usize> = Vec::new();
    for d in fresh {
        match owner.get(d) {
            Some(sis) => work.extend(sis.iter().copied()),
            None => return vec![true; n],
        }
    }
    dependents(solves, per_unit, work)
}

/// The units defining each display. A display may have several owners:
/// every unit holding a copy of one literal shape (`decl_units`).
fn owners(per_unit: &[Option<ProgramFacts>]) -> FxHashMap<String, Vec<usize>> {
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
fn dependents(solves: &[Solve], per_unit: &[Option<ProgramFacts>], mut work: Vec<usize>) -> Vec<bool> {
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

fn merge_unit(pf: &mut ProgramFacts, mi: usize, path: &str, u: ProgramFacts) {
    for mut t in u.templates {
        if t.module != Some(0) {
            continue;
        }
        t.module = Some(mi as u32);
        t.id = pf.templates.len() as crate::model::TmplId;
        pf.templates.push(t);
    }
    pf.sites.extend(u.sites.into_iter().map(|s| SiteFacts { module: mi as u32, ..s }));
    if let Some(b) = u.bindings.into_iter().next() {
        let into = &mut pf.bindings[mi];
        into.locals.extend(b.locals);
        into.this_of.extend(b.this_of);
    }
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
fn typed_param_verdicts(pf: &mut ProgramFacts) {
    let mut cand_of: FxHashMap<Vec<String>, Vec<usize>> = FxHashMap::default();
    for s in pf.sites.iter_mut() {
        let Some(keys) = s.guard_keys.take() else { continue };
        if !matches!(s.verdict, crate::Verdict::No(_)) || s.is_call {
            continue;
        }
        let tfs = &pf.templates;
        let cands = cand_of.entry(keys.clone()).or_insert_with(|| {
            (0..tfs.len())
                .filter(|&i| {
                    let t = &tfs[i];
                    t.class_id.is_some()
                        && keys.iter().all(|k| t.fields.iter().any(|f| &f.key == k && f.definite && f.data_field))
                })
                .collect()
        });
        if cands.is_empty() {
            continue;
        }
        let slot = |t: &TemplateFacts| {
            t.fields.iter().find(|f| f.key == s.key && f.definite && f.data_field).and_then(|f| f.slot)
        };
        let first = slot(&tfs[cands[0]]);
        if first.is_some() && cands.iter().all(|&i| tfs[i].prefix_stable && slot(&tfs[i]) == first) {
            s.receivers = cands.iter().map(|&i| tfs[i].display.clone()).collect();
            s.verdict = crate::Verdict::E0;
        }
    }
}
