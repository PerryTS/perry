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
    /// Its parent class when that class is another unit's (DECISIONS 104):
    /// the parent's display, its contract published beside this one. The
    /// chain inside the defining unit is in `methods`; past it, a reader
    /// finds inherited methods on the parent's stand-in, as the defining
    /// unit does.
    pub parent: Option<String>,
    /// Fields that hold exactly these functions of the defining unit, each
    /// a function contract (a literal's methods: `errorUtil.errToObj`);
    /// every other field holds unknown values for the reader.
    pub field_fns: Vec<(String, Vec<FnContract>)>,
    /// Fields that hold only instances of these layouts of the defining
    /// unit (key, their displays, whether primitives too): nested
    /// contracts (design §18.7, `Gen::nested_fields`).
    pub field_layouts: Vec<(String, Vec<String>, bool)>,
    /// Fields that hold other units' stand-ins (builtins, unknown values,
    /// primitives) and nothing of the defining unit: what they hold, as a
    /// value contract (DECISIONS 88), the reader seeds them with.
    pub field_values: Vec<(String, ValueContract)>,
    /// Fields that hold only the defining unit's typed arrays, none escaped
    /// or hard (DECISIONS 126): key, their one element kind, the primitives
    /// beside them. The reader holds one stand-in typed array per class and
    /// key; its census reports `ta <key> of <display>` for what it does.
    pub field_tas: Vec<(String, Option<u8>, u16)>,
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
    /// The prototype methods with a function contract (key, contract), and
    /// the constructor's (DECISIONS 57): their receiver is a contract
    /// parameter, so another unit's subclass instance can run them.
    pub method_fns: Vec<(String, FnContract)>,
    pub ctor_fn: Option<FnContract>,
    /// Its instances' birth keys in birth order and the definite ones, and
    /// whether its constructor or one up its chain leaks `this`, as its own
    /// unit computed them (`compute_birth_keys`, `ctor_phase`): a subclass
    /// in another unit takes them as its layout prefix, as a subclass in
    /// the same unit does.
    pub birth_keys: Vec<String>,
    pub definite: Vec<String>,
    pub ctor_leaks: bool,
    /// The primitive kinds the published binding may hold beside the
    /// class (DECISIONS 115): a nested class whose members name it binds
    /// itself through a one-element cell that holds `undefined` until the
    /// class is evaluated (DECISIONS 50). A reader's import holds them as
    /// the defining unit's binding does; a layout reached any other way
    /// has none.
    pub binding_prim: u16,
}

/// A published function as its callers see it (design §11.2): per
/// parameter what the callee does with everything reachable from the
/// argument (`model::MARK_*`: read, touched by a protocol, or used beyond:
/// then it escapes), whether its body reads `arguments` (`variadic`: all
/// arguments escape) or has a rest parameter (`rest`: the arguments past
/// the parameters escape), and the layouts it returns (`None`: ⊤).
/// DECISIONS 118: one key a contract function stores into parameter
/// `dst`'s argument (`reach`: into what it reaches). The reader stores the
/// arguments of the parameters `from` under it, ⊤ when `top`, and `prim`.
#[derive(Clone, Debug)]
pub(crate) struct StoreFlow {
    pub dst: u32,
    pub reach: bool,
    /// Into what the argument holds under this key (a keyed reach, design
    /// §18.4), not into all it reaches.
    pub via: Option<String>,
    pub key: String,
    pub from: Vec<u32>,
    pub top: bool,
    pub prim: u16,
}

#[derive(Clone, Debug)]
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
    /// What the result may hold through one named key of a parameter
    /// (`return p.k`, `u32::MAX`: the receiver): the reader loads that key
    /// of its argument, not everything the argument reaches (its
    /// prototype's methods, its class), as for `reach_keys` (§18.4).
    pub ret_reach_keys: Vec<(u32, String)>,
    /// The result may also be an unknown value (⊤ to the caller).
    pub ret_unknown: bool,
    /// The global builtins, program objects and arrays it may return
    /// (DECISIONS 94, as a value contract holds them).
    pub ret_value: ValueContract,
    /// The primitive kinds the result may be (`model::prim`), when `ret`
    /// names its layouts; with no layouts (⊤) any.
    pub ret_prim: u16,
    /// Its receiver is not used: an arrow function, or a body that never
    /// reads `this` (§18.2).
    pub lexical_this: bool,
    /// What the body does to its receiver and to what that reaches
    /// (`model::MARK_*`; ESCAPE when it has no receiver marker), and the
    /// keys it stores into it (`None`: a computed key) (DECISIONS 57).
    pub this_use: u8,
    pub this_reach: u8,
    pub this_stores: Option<Vec<String>>,
    /// The keys the body calls on its receiver that only another unit's
    /// subclass defines (DECISIONS 97): a reader runs `this.<key>(⊤)` on
    /// each receiver it admits, the call the defining unit cannot see.
    pub this_calls: Vec<String>,
    /// Per parameter, the keys the body stores into the argument (`None`:
    /// a computed key); a stored value escapes (DECISIONS 84).
    pub param_stores: Vec<Option<Vec<String>>>,
    /// What the body stores under a named key into an argument or into
    /// what it reaches: the arguments of other parameters, or an unknown
    /// value, or primitives (DECISIONS 118).
    pub param_flows: Vec<StoreFlow>,
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
    /// The global builtins it may hold (design §17.16 (a)): a builtin is one
    /// runtime object for the whole program, the same token in every unit.
    pub builtins: Vec<crate::model::BuiltinNs>,
    /// The program objects it may hold (DECISIONS 92: the CommonJS main
    /// record, `require.cache`, `require.extensions`): one runtime object
    /// each, the same in every unit; their fields are `<name>:<key>` cells.
    pub program: Vec<String>,
    /// The arrays of other units it may hold (DECISIONS 94), by template
    /// display: their elements are cell `arr:<display>`.
    pub arrays: Vec<String>,
}

impl ValueContract {
    /// The union of two contracts (decision 13's argument join).
    pub(crate) fn join(&mut self, b: &ValueContract) {
        for l in &b.layouts {
            if !self.layouts.iter().any(|x| x.display == l.display) {
                self.layouts.push(l.clone());
            }
        }
        for c in &b.classes {
            if !self.classes.iter().any(|x| x.display == c.display) {
                self.classes.push(c.clone());
            }
        }
        for f in &b.fns {
            if !self.fns.iter().any(|x| x.name == f.name) {
                self.fns.push(f.clone());
            }
        }
        for &n in &b.builtins {
            if !self.builtins.contains(&n) {
                self.builtins.push(n);
            }
        }
        for p in &b.program {
            if !self.program.contains(p) {
                self.program.push(p.clone());
            }
        }
        for a in &b.arrays {
            if !self.arrays.contains(a) {
                self.arrays.push(a.clone());
            }
        }
        self.unknown |= b.unknown;
        self.prim |= b.prim;
        self.program.sort();
        self.arrays.sort();
        self.layouts.sort_by(|x, y| x.display.cmp(&y.display));
        self.classes.sort_by(|x, y| x.display.cmp(&y.display));
        self.fns.sort_by(|x, y| x.name.cmp(&y.name));
        self.builtins.sort();
    }
}

impl ClassContract {
    /// The join of two units' contracts for one literal shape (DECISIONS
    /// 112): the HIR gives every object literal of one key set in a module
    /// one shape, and the units birthing it each publish what their own
    /// births hold. One runtime shape carries one fact set, so readers get
    /// what holds for every birth, as the whole program's one template.
    /// Idempotent and absorbing (`a ⊔ b = a` when `b ≤ a`): the driver joins
    /// the last round's entry with this round's owners.
    pub(crate) fn join(&mut self, b: &ClassContract) {
        self.binding_prim |= b.binding_prim;
        let (f, g) = (&mut self.facts, &b.facts);
        f.escape = f.escape.take().or_else(|| g.escape.clone());
        f.hard = f.hard.take().or_else(|| g.hard.clone());
        f.chain = f.chain.take().or_else(|| g.chain.clone());
        f.prefix_stable &= g.prefix_stable;
        f.fixed_layout &= g.fixed_layout;
        f.adds_keys = f.adds_keys.max(g.adds_keys);
        f.sabotaged |= g.sabotaged;
        common_prefix(&mut f.birth_keys, &g.birth_keys);
        for y in &g.fields {
            match f.fields.iter_mut().find(|x| x.key == y.key) {
                Some(x) => {
                    if x.slot != y.slot {
                        x.slot = None;
                    }
                    x.definite &= y.definite;
                    x.data_field &= y.data_field;
                    x.readonly &= y.readonly;
                    x.writers_ctor = x.writers_ctor.max(y.writers_ctor);
                    x.writers_post = x.writers_post.max(y.writers_post);
                    x.ta_stable &= y.ta_stable;
                    if x.ta_kind != y.ta_kind {
                        x.ta_kind = None;
                    }
                    let (k, l) = (&mut x.kind, &y.kind);
                    k.top |= l.top;
                    k.prims |= l.prims;
                    k.funcs = k.funcs.max(l.funcs);
                    k.builtin_objs = k.builtin_objs.max(l.builtin_objs);
                    for o in &l.objs {
                        if !k.objs.contains(o) {
                            k.objs.push(o.clone());
                        }
                    }
                    k.objs.sort();
                }
                None => {
                    let mut y = y.clone();
                    y.definite = false;
                    f.fields.push(y);
                }
            }
        }
        for x in f.fields.iter_mut() {
            if !g.fields.iter().any(|y| y.key == x.key) {
                x.definite = false;
            }
        }
        // A field both sides describe the same way keeps the union; a
        // field one side describes another way, or not at all, holds
        // unknown values for the reader.
        let mut fns = Vec::new();
        for (k, v) in &self.field_fns {
            if let Some((_, w)) = b.field_fns.iter().find(|(x, _)| x == k) {
                let mut v = v.clone();
                for c in w {
                    if !v.iter().any(|x| x.name == c.name) {
                        v.push(c.clone());
                    }
                }
                v.sort_by(|x, y| x.name.cmp(&y.name));
                fns.push((k.clone(), v));
            }
        }
        let mut lays = Vec::new();
        for (k, v, p) in &self.field_layouts {
            if let Some((_, w, q)) = b.field_layouts.iter().find(|(x, _, _)| x == k) {
                let mut v = v.clone();
                for d in w {
                    if !v.contains(d) {
                        v.push(d.clone());
                    }
                }
                v.sort();
                lays.push((k.clone(), v, *p || *q));
            }
        }
        let mut vals = Vec::new();
        for (k, v) in &self.field_values {
            if let Some((_, w)) = b.field_values.iter().find(|(x, _)| x == k) {
                let mut v = v.clone();
                v.join(w);
                vals.push((k.clone(), v));
            }
        }
        let mut tas = Vec::new();
        for (k, e, p) in &self.field_tas {
            if let Some((_, f, q)) = b.field_tas.iter().find(|(x, _, _)| x == k) {
                tas.push((k.clone(), if e == f { *e } else { None }, *p | *q));
            }
        }
        self.field_fns = fns;
        self.field_layouts = lays;
        self.field_values = vals;
        self.field_tas = tas;
        self.open_keys |= b.open_keys;
        self.ctor_leaks |= b.ctor_leaks;
        common_prefix(&mut self.birth_keys, &b.birth_keys);
        self.definite.retain(|k| b.definite.contains(k));
    }
}

/// Keeps the prefix `a` and `b` share.
fn common_prefix(a: &mut Vec<String>, b: &[String]) {
    let n = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    a.truncate(n);
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
    /// The function contracts by name, each its defining unit's
    /// (DECISIONS 106), kept across rounds as `layouts`.
    pub(crate) own_fns: FxHashMap<String, FnContract>,
    /// DECISIONS 107: per published method (contract name), the receivers
    /// the units running it published, joined; `this_args_top`: a unit
    /// over the bound may run any of them on anything.
    pub(crate) this_args: FxHashMap<String, ValueContract>,
    pub(crate) this_args_top: bool,
    /// The argument contracts of closed functions (decision 13), by
    /// (module path, name): the union over every unit's calls.
    pub(crate) args: FxHashMap<(String, String), ArgContract>,
    /// Store effects (decision 17): per literal layout or class display,
    /// the existing keys other units store into and the value contract of
    /// what they store (union over every unit; DECISIONS 73 is its `prim`,
    /// design §17.16 (c)).
    pub(crate) stores: FxHashMap<String, Vec<(String, ValueContract)>>,
    /// The keys of each module's namespace object (design §18.2), and
    /// whether it may hold any key (an `export *` from outside the program).
    pub(crate) ns_keys: FxHashMap<String, (Vec<String>, bool)>,
    /// The classes (defining module path, export name) a class of another
    /// unit names in its `extends` (DECISIONS 57): their constructor is a
    /// contract too, run on any receiver.
    pub(crate) extended: FxHashSet<(String, String)>,
    /// Cells (design §17.16 (b)): a runtime location every unit names the
    /// same way (`cjs:<path>`, the registry union `cjs:*`, a computed path
    /// `cjs:?`), the value contracts written, each with the units writing
    /// it (their keys, sorted). A reader joins the other units' writes.
    /// Grouped by contract (DECISIONS 113): every unit may write one cell
    /// (`global:*`), and a reader takes each distinct write once.
    pub(crate) cells: FxHashMap<String, Vec<(ValueContract, Vec<String>)>>,
    /// A unit solve went over the bound: what it writes is unknown, so
    /// every cell may hold an unknown value.
    pub(crate) cells_top: bool,
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

    pub(crate) fn ns_keys(&self, path: &str) -> Option<&(Vec<String>, bool)> {
        self.ns_keys.get(path)
    }

    pub(crate) fn broken(&self, display: &str) -> bool {
        self.broken.contains(display)
    }

    pub(crate) fn extended(&self, path: &str, name: &str) -> bool {
        self.extended
            .contains(&(path.to_string(), name.to_string()))
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
    p
}

/// The keys of each module's namespace object (design §18.2): its export
/// names, from the declarations (the same ones `Gen::link_exports` puts on
/// the namespace). `export *` adds the source module's keys but `default`;
/// a source outside the program makes the namespace open. Syntactic.
pub(crate) fn namespace_keys(sorted: &[(&str, &Module)]) -> FxHashMap<String, (Vec<String>, bool)> {
    let index: FxHashMap<&str, usize> = sorted
        .iter()
        .enumerate()
        .map(|(i, (p, _))| (*p, i))
        .collect();
    let mut own: Vec<(Vec<String>, Vec<Option<usize>>)> = Vec::with_capacity(sorted.len());
    for (_, m) in sorted {
        let mut keys: Vec<String> = Vec::new();
        let mut stars: Vec<Option<usize>> = Vec::new();
        for e in &m.exports {
            match e {
                Export::Named { exported, .. } | Export::ReExport { exported, .. } => {
                    keys.push(exported.clone())
                }
                Export::NamespaceReExport { name, .. } => keys.push(name.clone()),
                Export::ExportAll { source } => stars.push(
                    m.imports
                        .iter()
                        .find(|i| &i.source == source)
                        .and_then(|i| i.resolved_path.as_deref())
                        .and_then(|t| index.get(t).copied()),
                ),
            }
        }
        keys.extend(m.exported_functions.iter().map(|(n, _)| n.clone()));
        keys.extend(
            m.functions
                .iter()
                .filter(|f| f.is_exported)
                .map(|f| f.name.clone()),
        );
        keys.extend(
            m.classes
                .iter()
                .filter(|c| c.is_exported)
                .map(|c| c.name.clone()),
        );
        keys.extend(
            m.enums
                .iter()
                .filter(|e| e.is_exported)
                .map(|e| e.name.clone()),
        );
        keys.extend(m.exported_objects.iter().cloned());
        own.push((keys, stars));
    }
    let mut out = FxHashMap::default();
    for (i, (path, _)) in sorted.iter().enumerate() {
        let mut keys: FxHashSet<String> = own[i].0.iter().cloned().collect();
        let mut open = false;
        let mut seen = vec![i];
        let mut st: Vec<Option<usize>> = own[i].1.clone();
        while let Some(s) = st.pop() {
            let Some(s) = s else {
                open = true;
                continue;
            };
            if seen.contains(&s) {
                continue;
            }
            seen.push(s);
            keys.extend(own[s].0.iter().filter(|k| k.as_str() != "default").cloned());
            st.extend(own[s].1.iter().copied());
        }
        let mut keys: Vec<String> = keys.into_iter().collect();
        keys.sort();
        out.insert(path.to_string(), (keys, open));
    }
    out
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
    for si in 0..out.len() {
        let (path, module) = sorted[out[si].mi];
        let mut after: Vec<usize> = Vec::new();
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
    let ucfg = FactsConfig {
        unit: true,
        ..cfg.clone()
    };
    let flats: Vec<Option<Module>> = sorted
        .iter()
        .map(|(_, m)| crate::decl_units::run_once_flat(m))
        .collect();
    let solves = solves(sorted, &flats);
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
    let mut input = published_surface(&viewed, cfg);
    input.ns_keys = namespace_keys(sorted);
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
    // The layouts whose entry is their owner's, not a forwarded copy
    // (DECISIONS 106).
    let mut owned_layouts: FxHashSet<String> = FxHashSet::default();
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
                    published_layouts.insert(c.display.clone());
                    if u.layouts_fwd.contains(&c.display) {
                        if !owned_layouts.contains(&c.display) {
                            input.layouts.insert(c.display.clone(), c.clone());
                        }
                        continue;
                    }
                    owned_now.insert(c.display.clone());
                    // Several units birth one literal shape (DECISIONS 112):
                    // its entry is the join of their contracts, ascending
                    // from the last round's owned entry, so a reader between
                    // two owners reads at the fixpoint what the last one
                    // leaves. A class has one defining unit: its latest solve.
                    let e = match input.layouts.get(&c.display) {
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
                    input.layouts.insert(c.display.clone(), e);
                }
                for c in &u.own_fns_out {
                    published_fns.insert(c.name.clone());
                    input.own_fns.insert(c.name.clone(), c.clone());
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
                args.entry((path.to_string(), nm.clone()))
                    .or_default()
                    .join(ac);
            }
            // A closed class's constructions, from any module (§18.13).
            for (k, ac) in &u.ctor_args_out {
                args.entry(k.clone()).or_default().join(ac);
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
        // DECISIONS 107: the receivers of published methods, from every
        // unit's latest solve. The owners of a method whose join changed
        // are solved again.
        let mut this_args: FxHashMap<String, ValueContract> = FxHashMap::default();
        let mut this_top = false;
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
        // Decision 17: the store effects, from every unit's latest solve.
        // The owners of a layout whose effect keys changed perform them,
        // solved again with their dependents.
        let mut stores: FxHashMap<String, Vec<(String, ValueContract)>> = FxHashMap::default();
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
        let changed: Vec<String> = stores
            .iter()
            .filter(|(d, v)| format!("{:?}", input.stores.get(*d)) != format!("{:?}", Some(*v)))
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
        // Design §17.16 (b): the cells, from every unit's latest solve. The
        // units reading a cell whose writes changed are solved again.
        let mut cells: FxHashMap<String, Vec<(ValueContract, Vec<String>)>> = FxHashMap::default();
        let mut group: FxHashMap<(String, String), usize> = FxHashMap::default();
        for (si, s) in solves.iter().enumerate() {
            let u = per_unit[si].as_ref().expect("every unit solved");
            for (k, c) in &u.cells_out {
                let v = cells.entry(k.clone()).or_default();
                let i = *group.entry((k.clone(), format!("{c:?}"))).or_insert_with(|| {
                    v.push((c.clone(), Vec::new()));
                    v.len() - 1
                });
                v[i].1.push(s.key.clone());
            }
        }
        for v in cells.values_mut() {
            for (_, ws) in v.iter_mut() {
                ws.sort_unstable();
            }
            v.sort_by(|x, y| x.1.cmp(&y.1));
        }
        let cells_top = per_unit.iter().flatten().any(|u| u.stats.aborted);
        // A unit is solved again when what it READS of a cell changed: the
        // other writers' contracts (its own write is in its own solve,
        // DECISIONS 89), an unknown one or a unit over the bound being ⊤
        // that absorbs every other unknown. A reader forwarding the ⊤ it
        // was seeded with changes no other reader's view.
        let view = |cells: &FxHashMap<String, Vec<(ValueContract, Vec<String>)>>,
                    top: bool,
                    me: &str,
                    k: &str| {
            let others: Vec<&ValueContract> = cells
                .get(k)
                .into_iter()
                .flatten()
                .filter(|(_, ws)| !(ws.len() == 1 && ws[0] == me))
                .map(|(c, _)| c)
                .collect();
            let pure = |c: &ValueContract| {
                c.unknown
                    && c.layouts.is_empty()
                    && c.classes.is_empty()
                    && c.fns.is_empty()
                    && c.builtins.is_empty()
                    && c.program.is_empty()
                    && c.arrays.is_empty()
            };
            let top = top || others.iter().any(|c| pure(c));
            let mut v: Vec<String> = others
                .into_iter()
                .filter(|c| !pure(c))
                .map(|c| {
                    let mut c = c.clone();
                    if top {
                        c.unknown = false;
                        c.prim = 0;
                    }
                    format!("{c:?}")
                })
                .collect();
            v.sort_unstable();
            v.dedup();
            (top, v)
        };
        // A layout no unit published this round is gone; a unit that read
        // a layout which changed during the round read it before its
        // publisher solved, or the old one: solved again (DECISIONS 103).
        input.layouts.retain(|k, _| published_layouts.contains(k));
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
            .map(|(k, c)| (k.as_str(), digest(&Some(c))))
            .collect();
        let layouts_now: FxHashMap<&str, u64> = input
            .layouts
            .iter()
            .map(|(k, c)| (k.as_str(), digest(&Some(c))))
            .collect();
        compares.set(compares.get() + (fns_now.len() + layouts_now.len()) as u64);
        let fn_changed = |(k, h): &(String, u64)| {
            fns_now.get(k.as_str()).copied().unwrap_or(gone) != *h
        };
        for (si, u) in per_unit.iter().enumerate() {
            let u = u.as_ref().expect("every unit solved");
            // A unit over the bound keeps its result (`dependents`): what it
            // read changes nothing it publishes.
            if !u.stats.aborted && u.fns_read.iter().any(fn_changed) {
                seeds.push(si);
            }
        }
        let layout_changed = |(k, h): &(String, u64)| {
            layouts_now.get(k.as_str()).copied().unwrap_or(gone) != *h
        };
        for (si, u) in per_unit.iter().enumerate() {
            let u = u.as_ref().expect("every unit solved");
            // A unit over the bound keeps its result (`dependents`): what it
            // read changes nothing it publishes.
            if !u.stats.aborted && u.layouts_read.iter().any(layout_changed) {
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
            .filter(|k| format!("{:?}", input.cells.get(*k)) != format!("{:?}", cells.get(*k)))
            .map(|k| k.as_str())
            .collect();
        let top_changed = input.cells_top != cells_top;
        for (si, u) in per_unit.iter().enumerate() {
            let u = u.as_ref().expect("every unit solved");
            let me = solves[si].key.as_str();
            if u.cells_read.iter().any(|k| {
                (top_changed || changed.contains(k.as_str()))
                    && {
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
        pf.compares = compares.get();
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
        for (n, c) in &u.this_args_out {
            o.push((format!("this:{n}"), hash(&format!("{c:?}"))));
        }
        for (d, k, kinds) in &u.stores_out {
            o.push((format!("store:{d}.{k}"), hash(&format!("{kinds:?}"))));
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
            None => return vec![true; n],
        }
    }
    dependents(solves, per_unit, work)
}

/// What a reader saw of an entry read by name (a layout, a function
/// contract, DECISIONS 103/106): it is solved again when the entry differs.
pub(crate) fn digest<T: std::fmt::Debug>(v: &T) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{v:?}").hash(&mut h);
    h.finish()
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

fn merge_unit(pf: &mut ProgramFacts, mi: usize, path: &str, u: ProgramFacts) {
    for mut t in u.templates {
        if t.module != Some(0) {
            continue;
        }
        t.module = Some(mi as u32);
        t.id = pf.templates.len() as crate::model::TmplId;
        pf.templates.push(t);
    }
    pf.sites.extend(u.sites.into_iter().map(|s| SiteFacts {
        module: mi as u32,
        ..s
    }));
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
