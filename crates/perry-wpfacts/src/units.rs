//! Per-unit driver pieces (decision 151, design §11): what a unit
//! publishes to the other units of the program.

use crate::pts::{FxHashMap, FxHashSet};
use crate::{FactsConfig, ProgramFacts, SiteFacts, TemplateFacts};
use perry_hir::{Export, Import, ImportSpecifier, Module};

/// A published class as its importers see it (design §11.2): its template
/// and the facts its defining unit proved, its prototype methods (key,
/// function name) along the chain inside that unit, and its native parent.
#[derive(Clone, Debug, PartialEq)]
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
    /// Fields that hold only instances of these layouts of the defining
    /// unit (key, their displays, whether primitives too): nested
    /// contracts (design §18.7, `Gen::nested_fields`).
    pub field_layouts: Vec<(String, Vec<String>, bool)>,
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
    /// A closed class (design §18.13, `decl_units::closed_bindings`): its
    /// defining module's path and binding name, and its constructor's
    /// parameter count. Every unit constructing it (by name or through an
    /// instance's `constructor`) publishes what it passes under that key
    /// (`ProgramFacts::ctor_args_out`).
    pub closed: Option<(String, String, u32)>,
}

/// A published function as its callers see it (design §11.2): per
/// parameter what the callee does with everything reachable from the
/// argument (`model::MARK_*`: read, touched by a protocol, or used beyond:
/// then it escapes), whether its body reads `arguments` (`variadic`: all
/// arguments escape) or has a rest parameter (`rest`: the arguments past
/// the parameters escape), and the layouts it returns (`None`: ⊤).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FnContract {
    pub name: String,
    /// Per parameter: what the callee does to the argument itself
    /// (`model::MARK_*`), and to what the argument reaches (its fields,
    /// transitively). Two sets: reading `p.n` and converting it is no
    /// conversion of `p`. `reach` is what the callee does to what it reads
    /// under a computed key; `reach_keys` what it does to what it reads
    /// under each named key `p.k` (design §18.4: `p.left[i] = v` writes
    /// into `p.left`, not into `p`'s prototype).
    pub params: Vec<u8>,
    pub reach: Vec<u8>,
    pub reach_keys: Vec<Vec<(String, u8)>>,
    pub variadic: bool,
    pub rest: bool,
    pub ret: Option<Vec<String>>,
    /// The parameters whose argument the result may be, and those whose
    /// reachable values (fields, transitively) it may hold: result
    /// transfers, the caller's own objects come back, nothing escapes.
    pub ret_params: Vec<u32>,
    pub ret_reach: Vec<u32>,
    /// The result may also be an unknown value (⊤ to the caller).
    pub ret_unknown: bool,
    /// The primitive kinds the result may be (`model::prim`), when `ret`
    /// names its layouts; with no layouts (⊤) any.
    pub ret_prim: u16,
    /// Its receiver is not used: an arrow function, or a body that never
    /// reads `this` (§18.2).
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
#[derive(Clone, Debug, Default, PartialEq)]
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
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ArgContract {
    pub params: Vec<ValueContract>,
    pub top: bool,
}

/// Joins `b` into `a` as sets: what `b` holds that `a` lacks is appended,
/// so `a ⊔ a = a` (the driver compares last round's value with the join).
fn union_into<T: Clone + PartialEq>(a: &mut Vec<T>, b: &[T]) {
    for x in b {
        if !a.contains(x) {
            a.push(x.clone());
        }
    }
}

/// Joins entries keyed by `key`: an entry both hold is joined by `join`,
/// one only `b` holds is appended.
fn join_keyed<T: Clone, K: PartialEq>(
    a: &mut Vec<T>,
    b: &[T],
    key: impl Fn(&T) -> K,
    join: impl Fn(&mut T, &T),
) {
    for y in b {
        match a.iter_mut().find(|x| key(x) == key(y)) {
            Some(x) => join(x, y),
            None => a.push(y.clone()),
        }
    }
}

impl FnContract {
    /// The join of two descriptions of one function (by `name`): every
    /// claim the caller may rely on holds in both. The marks per
    /// parameter, variadic, the borrowed and returned parameters and an
    /// unknown result only grow; the result layouts grow, and a ⊤ result
    /// (`None`) absorbs them; the receiver is unused only if both say so.
    pub(crate) fn join(&mut self, o: &FnContract) {
        for (v, w) in [(&mut self.params, &o.params), (&mut self.reach, &o.reach)] {
            if v.len() < w.len() {
                v.resize(w.len(), 0);
            }
            for (x, y) in v.iter_mut().zip(w) {
                *x |= *y;
            }
        }
        // Per named key (§18.4): the marks grow, a key either reads is read.
        if self.reach_keys.len() < o.reach_keys.len() {
            self.reach_keys.resize(o.reach_keys.len(), Vec::new());
        }
        for (v, w) in self.reach_keys.iter_mut().zip(&o.reach_keys) {
            for (k, u) in w {
                match v.iter_mut().find(|(q, _)| q == k) {
                    Some((_, x)) => *x |= *u,
                    None => v.push((k.clone(), *u)),
                }
            }
            v.sort();
        }
        self.variadic |= o.variadic;
        self.rest |= o.rest;
        self.ret_prim |= o.ret_prim;
        match (&mut self.ret, &o.ret) {
            (Some(r), Some(s)) => union_into(r, s),
            (r, _) => *r = None,
        }
        union_into(&mut self.ret_params, &o.ret_params);
        union_into(&mut self.ret_reach, &o.ret_reach);
        self.ret_unknown |= o.ret_unknown;
        self.lexical_this &= o.lexical_this;
        if self.closed.is_none() {
            self.closed = o.closed.clone();
        }
    }
}

/// A static member of a class contract as its reader builds it
/// (`Gen::stand_in_class`): a function contract, a stub of the named
/// method, or an unknown value.
#[derive(Clone, PartialEq)]
enum StaticMember {
    Fn(FnContract),
    Stub(String),
    Top,
}

impl ClassContract {
    /// The join of two descriptions of one layout or class (by `display`;
    /// its name, class id, shape and native parent are its identity):
    /// every claim a reader may rely on holds in both. Facts weaken
    /// (`derive::TemplateFacts::join`); keys, methods and statics are the
    /// union, and a member the two describe differently is an unknown
    /// value; a field holds contract functions only where both say so;
    /// open keys and open statics only grow.
    pub(crate) fn join(&mut self, o: &ClassContract) {
        // Field functions: a field both describe holds the union of their
        // functions; a field one side's facts have without field functions
        // holds unknown values, so it has none in the join.
        let plain = |c: &ClassContract, k: &str| {
            c.facts.fields.iter().any(|f| f.key == k) && !c.field_fns.iter().any(|(q, _)| q == k)
        };
        let mut field_fns: Vec<(String, Vec<FnContract>)> = Vec::new();
        for (k, fs) in self.field_fns.iter().chain(o.field_fns.iter()) {
            if plain(self, k) || plain(o, k) {
                continue;
            }
            match field_fns.iter_mut().find(|(q, _)| q == k) {
                Some((_, v)) => join_keyed(v, fs, |f| f.name.clone(), FnContract::join),
                None => field_fns.push((k.clone(), fs.clone())),
            }
        }
        // Statics, as the reader builds them.
        let member = |c: &ClassContract, k: &str| -> Option<StaticMember> {
            let (_, m) = c.statics.iter().find(|(q, _)| q == k)?;
            Some(match c.static_fns.iter().find(|(q, _)| q == k) {
                Some((_, fc)) => StaticMember::Fn(fc.clone()),
                None => match m {
                    Some(f) => StaticMember::Stub(f.clone()),
                    None => StaticMember::Top,
                },
            })
        };
        let mut statics: Vec<(String, Option<String>)> = Vec::new();
        let mut static_fns: Vec<(String, FnContract)> = Vec::new();
        for (k, _) in self.statics.iter().chain(o.statics.iter()) {
            if statics.iter().any(|(q, _)| q == k) {
                continue;
            }
            let m = match (member(self, k), member(o, k)) {
                (Some(x), None) | (None, Some(x)) => x,
                (Some(StaticMember::Fn(mut f)), Some(StaticMember::Fn(g))) if f.name == g.name => {
                    f.join(&g);
                    StaticMember::Fn(f)
                }
                (Some(x), Some(y)) if x == y => x,
                _ => StaticMember::Top,
            };
            let slot = self
                .statics
                .iter()
                .chain(o.statics.iter())
                .find(|(q, _)| q == k)
                .and_then(|(_, m)| m.clone());
            match m {
                StaticMember::Fn(f) => {
                    statics.push((k.clone(), slot));
                    static_fns.push((k.clone(), f));
                }
                StaticMember::Stub(f) => statics.push((k.clone(), Some(f))),
                StaticMember::Top => statics.push((k.clone(), None)),
            }
        }
        // Nested layouts (§18.7), as the field functions: a field both
        // describe holds the union of their layouts (primitives too if
        // either says so); a field one side's facts have without them holds
        // unknown values, so it has none in the join.
        let plain_l = |c: &ClassContract, k: &str| {
            c.facts.fields.iter().any(|f| f.key == k)
                && !c.field_layouts.iter().any(|(q, _, _)| q == k)
        };
        let mut field_layouts: Vec<(String, Vec<String>, bool)> = Vec::new();
        for (k, ds, p) in self.field_layouts.iter().chain(o.field_layouts.iter()) {
            if plain_l(self, k) || plain_l(o, k) {
                continue;
            }
            match field_layouts.iter_mut().find(|(q, _, _)| q == k) {
                Some((_, v, q)) => {
                    union_into(v, ds);
                    *q |= *p;
                }
                None => field_layouts.push((k.clone(), ds.clone(), *p)),
            }
        }
        field_layouts.sort();
        // A closed class (§18.13) only where both say so, the same binding.
        if self.closed != o.closed {
            self.closed = None;
        }
        self.facts.join(&o.facts);
        union_into(&mut self.methods, &o.methods);
        self.field_fns = field_fns;
        self.field_layouts = field_layouts;
        self.open_keys |= o.open_keys;
        self.statics = statics;
        self.statics_open |= o.statics_open;
        self.static_fns = static_fns;
    }
}

impl ValueContract {
    /// The join of two value contracts: the layouts, classes and
    /// functions (each by identity, its description joined), an unknown
    /// value and the primitive kinds only grow.
    pub(crate) fn join(&mut self, o: &ValueContract) {
        join_keyed(
            &mut self.layouts,
            &o.layouts,
            |c| c.display.clone(),
            ClassContract::join,
        );
        join_keyed(
            &mut self.classes,
            &o.classes,
            |c| c.display.clone(),
            ClassContract::join,
        );
        join_keyed(&mut self.fns, &o.fns, |f| f.name.clone(), FnContract::join);
        self.unknown |= o.unknown;
        self.prim |= o.prim;
        self.layouts.sort_by(|x, y| x.display.cmp(&y.display));
        self.classes.sort_by(|x, y| x.display.cmp(&y.display));
        self.fns.sort_by(|x, y| x.name.cmp(&y.name));
    }
}

impl ArgContract {
    /// The lattice join (D16): `top`, the parameters and each parameter's
    /// value contract (`ValueContract::join`, descriptions included) only
    /// grow. Every component is a join in a finite lattice, so the driver's
    /// argument contracts, last round's joined with what the latest solves
    /// pass, ascend and stop changing.
    fn join(&mut self, o: &ArgContract) {
        self.top |= o.top;
        if self.params.len() < o.params.len() {
            self.params.resize(o.params.len(), ValueContract::default());
        }
        for (a, b) in self.params.iter_mut().zip(&o.params) {
            a.join(b);
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
    /// Modules whose namespace object itself reaches other units as a value
    /// no contract describes: a dynamic `import()` or an `export * as`
    /// re-export. A static namespace import is described (§18.2).
    ns_object: FxHashSet<String>,
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
    /// Store effects (decision 17): per literal layout or class display,
    /// the existing keys other units store into and the value contract of
    /// what they store (union over every unit; its kinds, DECISIONS 73).
    pub(crate) stores: FxHashMap<String, Vec<(String, ValueContract)>>,
    /// Write effects: per module-scope binding (its unit's path, its name)
    /// the value contract of what other units store into it (union over
    /// every unit, `Gen::local_write`).
    pub(crate) writes: FxHashMap<(String, String), ValueContract>,
    /// Each module's namespace object (design §18.2): its keys, where each
    /// resolves through re-exports (§18.14), and whether it may hold any key
    /// (an `export *` from outside the program).
    pub(crate) exports: Exports,
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
    /// What other units write into binding `name` of unit `path`.
    pub(crate) fn binding_write(&self, path: &str, name: &str) -> Option<&ValueContract> {
        self.writes.get(&(path.to_string(), name.to_string()))
    }

    pub(crate) fn stored_keys(&self, display: &str) -> &[(String, ValueContract)] {
        self.stores.get(display).map_or(&[], |v| v.as_slice())
    }

    pub(crate) fn eval_scopes(&self) -> bool {
        self.eval_scopes
    }

    /// Module `path`'s namespace object reaches other units undescribed.
    /// Interpreted code (decision 150) holds what escaped, the globals and
    /// what is thrown, as in the whole-program solve: no namespace object
    /// unless a direct eval can name a module's bindings (DECISIONS 65).
    pub(crate) fn ns_object(&self, path: &str) -> bool {
        self.eval_scopes || self.ns_object.contains(path)
    }

    pub(crate) fn ns_keys(&self, path: &str) -> Option<&ModExports> {
        self.exports.by_path.get(path)
    }

    /// Where `import { name } from path` leads (§18.14).
    pub(crate) fn resolve(&self, path: &str, name: &str) -> Option<Resolved<'_>> {
        self.exports.resolve(path, name)
    }

    pub(crate) fn broken(&self, display: &str) -> bool {
        self.broken.contains(display)
    }

    pub(crate) fn names(&self, path: &str, name: &str) -> bool {
        self.whole(path) || self.names.get(path).is_some_and(|n| n.contains(name))
    }
}

/// The module an `export … from source` of module `path` reads: the
/// driver's resolution (`FactsConfig::export_sources`), else an import of
/// the same source, as `Gen::resolve_source`. A re-export declares no import
/// of its source (`export * as X from "./self"`), so the imports alone miss
/// it, and the surface, the namespace keys and the order must not.
fn reexport_source<'i>(
    es: &crate::ExportSources,
    path: &str,
    imports: impl IntoIterator<Item = &'i Import>,
    source: &str,
) -> Option<String> {
    if let Some(p) = es.get(path).and_then(|m| m.get(source)) {
        return Some(p.clone());
    }
    imports
        .into_iter()
        .find(|i| i.source == source)
        .and_then(|i| i.resolved_path.clone())
}

pub(crate) fn published_surface(
    sorted: &[(&str, Vec<&Import>, &[Export])],
    cfg: &FactsConfig,
    ex: &Exports,
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
            reexport_source(&cfg.export_sources, path, imports.iter().copied(), source)
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
            // `import()` hands over the namespace object itself, also where
            // the driver folds it into a static import of the same module.
            if imp.is_dynamic || imp.is_dynamic_target {
                p.whole.insert(t.clone());
                p.ns_object.insert(t.clone());
            }
            for s in &imp.specifiers {
                match s {
                    ImportSpecifier::Named { imported, .. } => {
                        p.names
                            .entry(t.clone())
                            .or_default()
                            .insert(imported.clone());
                    }
                    ImportSpecifier::Default { .. } => {
                        p.names
                            .entry(t.clone())
                            .or_default()
                            .insert("default".into());
                    }
                    ImportSpecifier::Namespace { .. } => {
                        p.whole.insert(t.clone());
                    }
                }
            }
        }
        for e in exports.iter() {
            match e {
                Export::ReExport {
                    source, imported, ..
                } => {
                    if let Some(t) = resolve(source) {
                        p.names.entry(t).or_default().insert(imported.clone());
                    }
                }
                Export::ExportAll { source } => {
                    if let Some(t) = resolve(source) {
                        p.whole.insert(t);
                    }
                }
                Export::NamespaceReExport { source, .. } => {
                    if let Some(t) = resolve(source) {
                        p.whole.insert(t.clone());
                        p.ns_object.insert(t);
                    }
                }
                Export::Named { .. } => {}
            }
        }
    }
    // A name a module re-exports is named where it is declared (§18.14):
    // published there, as the importer reads its contract there; a
    // re-exported namespace is whole there.
    let mut add: Vec<(String, String)> = Vec::new();
    let mut whole: Vec<String> = p.whole.iter().cloned().collect();
    for (t, names) in &p.names {
        for n in names {
            match ex.resolve(t, n) {
                Some(Resolved::Binding(d, m)) if d != t || m != n => {
                    add.push((d.to_string(), m.to_string()))
                }
                Some(Resolved::Namespace(d)) => whole.push(d.to_string()),
                _ => {}
            }
        }
    }
    let mut seen: FxHashSet<String> = FxHashSet::default();
    while let Some(t) = whole.pop() {
        if !seen.insert(t.clone()) {
            continue;
        }
        p.whole.insert(t.clone());
        let Some(me) = ex.by_path.get(&t) else {
            continue;
        };
        for (k, target) in &me.keys {
            match target {
                ExportTarget::Binding(d, m) => {
                    let d = &ex.paths[*d as usize];
                    if *d != t || m != k {
                        add.push((d.clone(), m.clone()));
                    }
                }
                ExportTarget::Namespace(d) => whole.push(ex.paths[*d as usize].clone()),
                ExportTarget::Unknown => {}
            }
        }
    }
    for (d, m) in add {
        p.names.entry(d).or_default().insert(m);
    }
    p
}

/// Where an export name of a module leads (ECMAScript ResolveExport, design
/// §18.14): a binding some module declares, a module's namespace object
/// (`export * as n from`, `import * as n; export { n }`), or unknown.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ExportTarget {
    /// Binding `.1` that module `.0` (an index into `Exports::paths`) declares.
    Binding(u32, String),
    /// The namespace object of module `.0`.
    Namespace(u32),
    /// A source outside the program, or a name two `export *` sources bind
    /// differently (the namespace leaves it out, a named import of it is a
    /// link error): nothing is known of it.
    Unknown,
}

/// A module's namespace (design §18.2): its export names, each with where
/// it resolves, sorted by name; open when an `export *` source in reach is
/// outside the program (any other key may be there).
#[derive(Clone, Debug, Default)]
pub(crate) struct ModExports {
    pub(crate) keys: Vec<(String, ExportTarget)>,
    pub(crate) open: bool,
}

impl ModExports {
    pub(crate) fn get(&self, k: &str) -> Option<&ExportTarget> {
        self.keys
            .binary_search_by(|(x, _)| x.as_str().cmp(k))
            .ok()
            .map(|i| &self.keys[i].1)
    }

    pub(crate) fn has(&self, k: &str) -> bool {
        self.get(k).is_some()
    }
}

/// Every module's namespace, by path; `paths` names the modules targets
/// point at.
#[derive(Clone, Debug, Default)]
pub(crate) struct Exports {
    pub(crate) paths: Vec<String>,
    pub(crate) by_path: FxHashMap<String, ModExports>,
}

/// What an import of a name from a module binds (`Published::resolve`).
#[derive(Debug, PartialEq)]
pub(crate) enum Resolved<'a> {
    /// Binding `.1` declared by module `.0`: its contract is keyed so.
    Binding(&'a str, &'a str),
    /// The namespace object of module `.0`.
    Namespace(&'a str),
    Unknown,
}

impl Exports {
    /// The other modules declaring a binding module `m`'s namespace holds,
    /// through the namespaces it holds too (`m` itself left out).
    pub(crate) fn reach(&self, m: usize) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        let mut seen: FxHashSet<usize> = FxHashSet::default();
        let mut todo = vec![m];
        while let Some(x) = todo.pop() {
            if !seen.insert(x) {
                continue;
            }
            let Some(me) = self.by_path.get(&self.paths[x]) else {
                continue;
            };
            for (_, t) in &me.keys {
                match *t {
                    ExportTarget::Binding(d, _) if d as usize != m => out.push(d as usize),
                    ExportTarget::Namespace(d) => todo.push(d as usize),
                    _ => {}
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Where `import { name } from path` leads: `None` when `path` has no
    /// such key (no export the declarations name: its contract, if any, is
    /// keyed by `path` itself).
    pub(crate) fn resolve<'s>(&'s self, path: &str, name: &str) -> Option<Resolved<'s>> {
        Some(match self.by_path.get(path)?.get(name)? {
            ExportTarget::Binding(m, n) => Resolved::Binding(&self.paths[*m as usize], n),
            ExportTarget::Namespace(m) => Resolved::Namespace(&self.paths[*m as usize]),
            ExportTarget::Unknown => Resolved::Unknown,
        })
    }
}

/// One module's own export entries, as ResolveExport reads them.
enum Own<'m> {
    /// Declared by the module under its export name.
    Here,
    /// An indirect export: name `.1` of module `.0` (`export { a } from`,
    /// `import { a }; export { a }`); `None`: a module outside the program.
    Indirect(Option<usize>, &'m str),
    /// The namespace of module `.0` (`None`: outside the program).
    Ns(Option<usize>),
}

struct Resolver<'m> {
    own: Vec<FxHashMap<&'m str, Own<'m>>>,
    stars: Vec<Vec<Option<usize>>>,
    /// Resolutions reached without cutting a cycle: the same from every
    /// resolve set.
    memo: Vec<FxHashMap<&'m str, Option<ExportTarget>>>,
}

impl<'m> Resolver<'m> {
    /// ResolveExport(m, name) (ECMA-262 16.2.1.6.3): `None` when not
    /// found; `.1` when a cycle was cut on the way (the result then
    /// depends on the resolve set and is not memoized).
    fn resolve(
        &mut self,
        m: usize,
        name: &'m str,
        set: &mut Vec<(usize, &'m str)>,
    ) -> (Option<ExportTarget>, bool) {
        if let Some(r) = self.memo[m].get(name) {
            return (r.clone(), false);
        }
        if set.contains(&(m, name)) {
            return (None, true);
        }
        set.push((m, name));
        let mut cyc = false;
        let r = match self.own[m].get(name) {
            Some(Own::Here) => Some(ExportTarget::Binding(m as u32, name.to_string())),
            Some(Own::Ns(Some(t))) => Some(ExportTarget::Namespace(*t as u32)),
            Some(Own::Ns(None) | Own::Indirect(None, _)) => Some(ExportTarget::Unknown),
            Some(&Own::Indirect(Some(t), imported)) => {
                let (r, c) = self.resolve(t, imported, set);
                cyc |= c;
                // An indirect export that resolves nowhere is a link error.
                Some(r.unwrap_or(ExportTarget::Unknown))
            }
            // `export *` never re-exports `default`.
            None if name == "default" => None,
            None => {
                let mut acc: Option<ExportTarget> = None;
                for i in 0..self.stars[m].len() {
                    let r = match self.stars[m][i] {
                        None => Some(ExportTarget::Unknown),
                        Some(s) => {
                            let (r, c) = self.resolve(s, name, set);
                            cyc |= c;
                            r
                        }
                    };
                    acc = match (acc, r) {
                        (a, None) => a,
                        (None, b) => b,
                        (Some(a), Some(b)) if a == b => Some(a),
                        // Two different bindings (ambiguous), or a source
                        // outside the program that may bind it too.
                        _ => Some(ExportTarget::Unknown),
                    };
                }
                acc
            }
        };
        set.pop();
        if !cyc {
            self.memo[m].insert(name, r.clone());
        }
        (r, cyc)
    }
}

/// Each module's namespace (design §18.2, §18.14): its export names
/// (GetExportedNames: its own, then every `export *` source's but
/// `default`, a revisited module ending the walk) and where each resolves
/// (ResolveExport), from the HIR's import and export declarations.
/// Compiler-internal: nothing of it is emitted.
pub(crate) fn module_exports(sorted: &[(&str, &Module)], es: &crate::ExportSources) -> Exports {
    let index: FxHashMap<&str, usize> = sorted
        .iter()
        .enumerate()
        .map(|(i, (p, _))| (*p, i))
        .collect();
    let mut own: Vec<FxHashMap<&str, Own>> = Vec::with_capacity(sorted.len());
    let mut stars: Vec<Vec<Option<usize>>> = Vec::with_capacity(sorted.len());
    for (path, m) in sorted {
        let module_of = |p: Option<String>| p.and_then(|t| index.get(t.as_str()).copied());
        // The module's import bindings: an export of one is indirect.
        let mut imported: FxHashMap<&str, Own> = FxHashMap::default();
        for imp in m.imports.iter().filter(|i| !i.type_only) {
            let t = || module_of(imp.resolved_path.clone());
            for sp in &imp.specifiers {
                let (local, o) = match sp {
                    ImportSpecifier::Named { imported, local } => {
                        (local, Own::Indirect(t(), imported.as_str()))
                    }
                    ImportSpecifier::Default { local } => (local, Own::Indirect(t(), "default")),
                    ImportSpecifier::Namespace { local } => (local, Own::Ns(t())),
                };
                imported.insert(local.as_str(), o);
            }
        }
        let mut o: FxHashMap<&str, Own> = FxHashMap::default();
        let mut st: Vec<Option<usize>> = Vec::new();
        for e in &m.exports {
            let src = |source: &str| module_of(reexport_source(es, path, &m.imports, source));
            match e {
                Export::Named { local, exported } => {
                    let v = match imported.get(local.as_str()) {
                        Some(&Own::Indirect(t, n)) => Own::Indirect(t, n),
                        Some(&Own::Ns(t)) => Own::Ns(t),
                        _ => Own::Here,
                    };
                    o.insert(exported.as_str(), v);
                }
                Export::ReExport {
                    source,
                    imported,
                    exported,
                } => {
                    o.insert(
                        exported.as_str(),
                        Own::Indirect(src(source), imported.as_str()),
                    );
                }
                Export::NamespaceReExport { source, name } => {
                    o.insert(name.as_str(), Own::Ns(src(source)));
                }
                Export::ExportAll { source } => st.push(src(source)),
            }
        }
        let decls = m
            .exported_functions
            .iter()
            .map(|(n, _)| n)
            .chain(
                m.functions
                    .iter()
                    .filter(|f| f.is_exported)
                    .map(|f| &f.name),
            )
            .chain(m.classes.iter().filter(|c| c.is_exported).map(|c| &c.name))
            .chain(m.enums.iter().filter(|e| e.is_exported).map(|e| &e.name))
            .chain(m.exported_objects.iter());
        for n in decls {
            o.entry(n.as_str()).or_insert(Own::Here);
        }
        own.push(o);
        stars.push(st);
    }
    let mut r = Resolver {
        own,
        stars,
        memo: (0..sorted.len()).map(|_| FxHashMap::default()).collect(),
    };
    let mut by_path = FxHashMap::default();
    for (i, (path, _)) in sorted.iter().enumerate() {
        // GetExportedNames.
        let mut names: FxHashSet<&str> = r.own[i].keys().copied().collect();
        let mut open = false;
        let mut seen = vec![i];
        let mut todo: Vec<Option<usize>> = r.stars[i].clone();
        while let Some(s) = todo.pop() {
            let Some(s) = s else {
                open = true;
                continue;
            };
            if seen.contains(&s) {
                continue;
            }
            seen.push(s);
            names.extend(r.own[s].keys().copied().filter(|k| *k != "default"));
            todo.extend(r.stars[s].iter().copied());
        }
        let mut keys: Vec<(String, ExportTarget)> = names
            .into_iter()
            .filter_map(|k| {
                let t = r.resolve(i, k, &mut Vec::new()).0?;
                Some((k.to_string(), t))
            })
            .collect();
        keys.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        by_path.insert(path.to_string(), ModExports { keys, open });
    }
    Exports {
        paths: sorted.iter().map(|(p, _)| p.to_string()).collect(),
        by_path,
    }
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
fn owner_of(out: &[Solve], t: &std::ops::Range<usize>, name: &str) -> Option<usize> {
    t.clone().find(|&si| {
        out[si].view.exports.iter().any(|e| match e {
            Export::Named { exported, .. } | Export::ReExport { exported, .. } => exported == name,
            Export::NamespaceReExport { name: n, .. } => n == name,
            _ => false,
        })
    })
}

fn solves<'a>(
    sorted: &[(&'a str, &'a Module)],
    flats: &'a [Option<Module>],
    es: &crate::ExportSources,
    ex: &Exports,
) -> Vec<Solve<'a>> {
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
        for (k, (view, deps)) in crate::decl_units::views(path, module)
            .into_iter()
            .enumerate()
        {
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
                let Some(&t) = reexport_source(es, path, std::iter::empty(), source)
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
        .map(|(_, m)| crate::decl_units::run_once_flat(m, cfg.cjs_closed))
        .collect();
    let exports = module_exports(sorted, &cfg.export_sources);
    let solves = solves(sorted, &flats, &cfg.export_sources, &exports);
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
                // Several units describe one layout (literals of one shape
                // in different units): a reader relies only on what all of
                // them say, and a field holds every unit's functions.
                for c in &u.layouts_out {
                    match input.layouts.get_mut(&c.display) {
                        Some(have) => have.join(c),
                        None => {
                            input.layouts.insert(c.display.clone(), c.clone());
                        }
                    }
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
        for (si, s) in solves.iter().enumerate() {
            let path = sorted[s.mi].0;
            let u = per_unit[si].as_ref().expect("every unit solved");
            if u.stats.aborted {
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
        // Decision 17: the store effects: last round's joined with what
        // every unit's latest solve stores (D16), so they only grow too.
        // The owners of a layout whose effect keys grew perform them,
        // solved again with their dependents.
        let mut stores = input.stores.clone();
        for u in per_unit.iter().flatten() {
            for (d, k, vc) in &u.stores_out {
                let v = stores.entry(d.clone()).or_default();
                match v.iter_mut().find(|(k2, _)| k2 == k) {
                    Some((_, e)) => e.join(vc),
                    None => v.push((k.clone(), vc.clone())),
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
            for (si, d) in dependents(&solves, &per_unit, seeds)
                .into_iter()
                .enumerate()
            {
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
        for ((p, n), c) in &u.ctor_args_out {
            o.push((format!("args:{p}:{n}"), hash(&format!("{c:?}"))));
        }
        for (d, k, vc) in &u.stores_out {
            o.push((format!("store:{d}.{k}"), hash(&format!("{vc:?}"))));
        }
        for ((p, n), vc) in &u.writes_out {
            o.push((format!("write:{p}.{n}"), hash(&format!("{vc:?}"))));
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
    }
    lines
}

/// The units a newly broken contract changes (design §11.3): the unit
/// defining it (a template it owns, or a function it publishes) and every
/// unit depending on that one, transitively. A unit over the bound claims
/// and publishes nothing whatever its inputs, so it keeps its result and
/// passes no change on: solving it again would only pay the bound again. A
/// display no unit defines makes every unit dirty.
fn resolve_again(
    solves: &[Solve],
    per_unit: &[Option<ProgramFacts>],
    fresh: &[String],
) -> Vec<bool> {
    let n = solves.len();
    let owner = owners(per_unit);
    let mut work: Vec<usize> = Vec::new();
    for d in fresh {
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
fn dependents(
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
fn program_bodies(bodies: &mut Vec<crate::bindings::BodyRef>, mi: usize) {
    if bodies.iter().any(|b| b.module != 0) {
        bodies.clear();
    }
    for b in bodies.iter_mut() {
        b.module = mi as u32;
    }
}

fn merge_unit(
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
        into.this_of.extend(
            b.this_of
                .into_iter()
                .filter(|(id, _)| view.binding_funcs.contains(id)),
        );
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
fn typed_param_verdicts(pf: &mut ProgramFacts) {
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
fn unique_bindings<T>(values: &mut [(u32, T)], module: &str, kind: &str) {
    values.sort_by_key(|(id, _)| *id);
    for pair in values.windows(2) {
        assert_ne!(
            pair[0].0, pair[1].0,
            "per-unit facts: duplicate {kind} {} in {module}; cloned bodies must have distinct binding identities",
            pair[0].0,
        );
    }
}

#[cfg(test)]
mod binding_collision_tests {
    #[test]
    #[should_panic(expected = "duplicate LocalId 7")]
    fn duplicate_local_is_an_internal_compiler_error() {
        super::unique_bindings(&mut [(7, ()), (7, ())], "clones.ts", "LocalId");
    }

    #[test]
    #[should_panic(expected = "duplicate this-of FuncId 9")]
    fn duplicate_this_is_an_internal_compiler_error() {
        super::unique_bindings(&mut [(9, ()), (9, ())], "clones.ts", "this-of FuncId");
    }
}
