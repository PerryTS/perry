//! Late keys (OBJECT_MODEL_2026-10-13.md §3.2: slot or overflow per key per
//! family under a width budget; the 2026-10-15 ruling on static-name stores).
//!
//! A key a static-name store writes after construction (`node.flowNode = x`)
//! gets a slot in family F's layout, a hole until written, when the store's
//! receiver may be an F object. Nothing on main proves the receivers of
//! library code (tsc's factory functions take untyped values), so "may" is a
//! census rule over the program's own text:
//!
//! * a receiver group is one local binding of a module (every function and
//!   closure), or `this` in the methods of a class;
//! * a local group may be F when F explains as much of the group's
//!   constructor-key usage as any family does, and a group that touches a
//!   single constructor key counts only when all families with that key are
//!   alike (`group_families`): a group of another family that only shares
//!   common names (`flags`, `id`, `parent`) does not count; a `this` group of
//!   class C may be F when C is on F's chain;
//! * a local initialised by a direct call of a function that returns one
//!   local carries that local's stores first (`createBaseDeclaration` stores
//!   `symbol`, `localSymbol`, then its caller stores the rest).
//!
//! Each family takes its candidates by the number of store sites, up to the
//! width budget, after its parent family's late keys (which keep the parent's
//! slots). The late keys' order in the key sequence is the order the stores
//! have in the groups, ranked by pairwise precedence; it is the enumeration
//! order of an object whose fills follow it, and the runtime moves an object
//! whose writes leave it to its overflow (`perry-runtime`
//! `object/layout_desc.rs`). Soundness never rests on the census: a late
//! slot is absent until written, and a key the census missed takes the
//! overflow.

use perry_hir::fx::FxHashMap;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use perry_hir::{Expr, Module as HirModule, Stmt};

use super::{explain, visit_stmt, visit_stmts, FamilyKey, RawFamily};

/// Keys a family's layout may hold beyond its constructor keys
/// (`PERRY_LAYOUT_LATE_BUDGET` overrides it for measurement; 0 = none).
/// Every slot widens every instance and every overflow's fixed prefix: tsc
/// (2026-10-15, instructions:u / RSS) 22.81G / 248 MB at 0, 22.91G / 257 MB
/// at 8, 23.28G / 256 MB at 16, 26.09G / 335 MB at 24, 28.83G / 381 MB at 48.
const LATE_BUDGET: usize = 8;

pub(super) fn late_budget() -> usize {
    std::env::var("PERRY_LAYOUT_LATE_BUDGET")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(LATE_BUDGET)
}

/// Can `k` be a late key at all (a plain string key a static store names)?
fn late_key_ok(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= 64
        && k != "__proto__"
        && k != "constructor"
        && !k.starts_with('#')
        && !k.bytes().all(|b| b.is_ascii_digit())
}

/// One receiver group's static keys.
#[derive(Default, Debug)]
pub(super) struct Group {
    /// Module-local binding identity, retained only for code selection.
    local: Option<u32>,
    /// Every key read or written through it.
    keys: BTreeSet<String>,
    /// Keys stored through it, in first-store order.
    stores: Vec<String>,
    /// Store sites per key.
    sites: FxHashMap<String, u32>,
    /// The local is a new object: initialised by `new`, or by a factory call
    /// whose callee returns one of its own locals (the stores of a creation
    /// group come before any later phase's).
    creation: bool,
    /// The number literals of the arguments of the call (or `new`) that
    /// initialises the local, by position (`None`: not a number literal).
    /// A family whose constructor stores a parameter into a constructor key
    /// splits its creation groups by the constant they pass for it
    /// ([`add_variants`]).
    pub(super) init_literals: Vec<Option<f64>>,
    /// The classes a factory-initialised local is a fresh instance of
    /// ([`Factories`]); empty when unknown. A group with classes belongs to
    /// their families and to no other.
    pub(super) classes: BTreeSet<u32>,
}

/// The number literal `e` is, if any.
fn number_literal(e: &Expr) -> Option<f64> {
    match e {
        Expr::Integer(i) => Some(*i as f64),
        Expr::Number(n) if n.is_finite() => Some(*n),
        _ => None,
    }
}

fn note(g: &mut Group, key: &str, store: bool) {
    g.keys.insert(key.to_string());
    if store {
        if !g.stores.iter().any(|k| k == key) {
            g.stores.push(key.to_string());
        }
        *g.sites.entry(key.to_string()).or_default() += 1;
    }
}

#[derive(Default)]
struct Census {
    locals: RefCell<FxHashMap<u32, Group>>,
    thises: RefCell<FxHashMap<u32, Group>>,
    /// Local -> the function it binds.
    fn_of_local: RefCell<FxHashMap<u32, u32>>,
    /// Local -> the callee of the call that initialises it.
    init_call: RefCell<FxHashMap<u32, Expr>>,
    /// Function -> the one local it returns (`None`: more than one).
    returns: RefCell<FxHashMap<u32, Option<u32>>>,
    /// Locals initialised by `new`.
    born: RefCell<BTreeSet<u32>>,
    /// Local -> the number literals of its initialising call's arguments.
    init_literals: RefCell<FxHashMap<u32, Vec<Option<f64>>>>,
    /// Local -> its initialising call (the whole call: [`Factories`]).
    init_expr: RefCell<FxHashMap<u32, Expr>>,
    /// Factory resolution facts ([`Factories`]).
    factories: RefCell<Factories>,
    /// Closed-shape literal class -> its keys, in order.
    anon_fields: FxHashMap<String, Vec<String>>,
}

/// What a name-level factory resolution needs (object model §3.2: a family
/// is a creation point, and a factory that allocates and returns the object
/// is part of it): which classes a call's result can be a fresh instance of.
/// tsc's `baseFactory2.createBaseIdentifierNode(80)` returns
/// `new (IdentifierConstructor2 || (IdentifierConstructor2 =
/// objectAllocator.getIdentifierConstructor()))(kind, -1, -1)`, and
/// `getIdentifierConstructor: () => IdentifierObject`. A method call resolves
/// by its property name to every function bound to that name in the module
/// (object-literal properties and named functions). The answer only decides
/// which family's layout a creation group's keys go into (width and slots);
/// no semantics rest on it.
#[derive(Default)]
struct Factories {
    /// Function -> the expressions it returns (the interesting forms only).
    returns: FxHashMap<u32, Vec<Expr>>,
    /// Property or function name -> the functions bound to it.
    named: FxHashMap<String, Vec<u32>>,
    /// Property name -> the locals bound to it (`{ f }` before `f`'s
    /// hoisted function declaration is visited: resolved at use).
    named_locals: FxHashMap<String, Vec<u32>>,
    /// Local -> the values stored into it (the interesting forms only).
    assigns: FxHashMap<u32, Vec<Expr>>,
}

/// Is `e` a form [`Factories`] follows (kept by clone)?
fn factory_form(e: &Expr) -> bool {
    matches!(
        e,
        Expr::New { .. }
            | Expr::NewDynamic { .. }
            | Expr::Call { .. }
            | Expr::ClassRef(_)
            | Expr::ClassExprFresh { .. }
            | Expr::LocalGet(_)
            | Expr::LocalSet(..)
            | Expr::Logical { .. }
    )
}

impl Factories {
    fn callee_fns(&self, callee: &Expr, fn_of_local: &FxHashMap<u32, u32>) -> Vec<u32> {
        match callee {
            Expr::FuncRef(f) => vec![*f],
            Expr::LocalGet(l) => fn_of_local.get(l).map(|f| vec![*f]).unwrap_or_default(),
            Expr::PropertyGet { property, .. } => {
                let mut fns = self.named.get(property).cloned().unwrap_or_default();
                for l in self.named_locals.get(property).into_iter().flatten() {
                    if let Some(f) = fn_of_local.get(l) {
                        fns.push(*f);
                    }
                }
                fns
            }
            _ => Vec::new(),
        }
    }

    /// The class names a fresh instance `e` evaluates to can be of.
    fn instance_classes(
        &self,
        e: &Expr,
        cx: &FactoryCx<'_>,
        depth: u32,
        out: &mut BTreeSet<String>,
    ) {
        self.walk_classes(e, cx, FactoryMode::Instance, depth, out);
    }

    /// Expand the bounded factory graph by distance, rather than by path.
    /// A common method name can resolve to many functions whose returns name
    /// that same method again. Recursive expansion visits every path through
    /// those diamonds, even though the result is a union of class names.
    /// Coalesce function/local vertices within each depth and query mode.
    /// Keep depths separate: a vertex reached by a longer path has less
    /// remaining reach, and constructor values have a different depth limit.
    /// Nothing survives this query and no class answer is memoized.
    /// Returns the number of expression visits for the work-bound regression.
    fn walk_classes<'a>(
        &'a self,
        e: &'a Expr,
        cx: &FactoryCx<'a>,
        mode: FactoryMode,
        depth: u32,
        out: &mut BTreeSet<String>,
    ) -> usize {
        let mut frontier = vec![(mode, e)];
        let mut work = 0;
        for depth in depth..=8 {
            let mut next = Vec::new();
            let mut functions = BTreeSet::new();
            let mut locals = BTreeSet::new();
            for (mode, e) in frontier {
                if depth > mode.depth_limit() {
                    continue;
                }
                work += 1;
                match (mode, e) {
                    (FactoryMode::Instance, Expr::New { class_name, .. }) => {
                        out.insert(class_name.clone());
                    }
                    (FactoryMode::Instance, Expr::NewDynamic { callee, .. }) => {
                        next.push((FactoryMode::Constructor, callee.as_ref()));
                    }
                    (_, Expr::Call { callee, .. }) => {
                        functions.extend(
                            self.callee_fns(callee, cx.fn_of_local)
                                .into_iter()
                                .map(|f| (mode, f)),
                        );
                    }
                    (FactoryMode::Constructor, Expr::ClassRef(name)) => {
                        out.insert(name.clone());
                    }
                    (FactoryMode::Constructor, Expr::ClassExprFresh { template, .. }) => {
                        out.insert(template.clone());
                    }
                    (FactoryMode::Constructor, Expr::LocalGet(local))
                        if cx.class_of_local.contains_key(local) =>
                    {
                        out.insert(cx.class_of_local[local].clone());
                    }
                    (_, Expr::LocalGet(local)) => {
                        locals.insert((mode, *local));
                    }
                    (FactoryMode::Constructor, Expr::LocalSet(_, value)) => {
                        next.push((mode, value.as_ref()));
                    }
                    (FactoryMode::Constructor, Expr::Logical { left, right, .. }) => {
                        next.push((mode, left.as_ref()));
                        next.push((mode, right.as_ref()));
                    }
                    _ => {}
                }
            }
            for (mode, function) in functions {
                next.extend(
                    self.returns
                        .get(&function)
                        .into_iter()
                        .flatten()
                        .map(|value| (mode, value)),
                );
            }
            for (mode, local) in locals {
                match mode {
                    FactoryMode::Instance => {
                        if let Some(init) = cx.init_expr.get(&local) {
                            next.push((mode, init));
                        }
                    }
                    FactoryMode::Constructor => next.extend(
                        self.assigns
                            .get(&local)
                            .into_iter()
                            .flatten()
                            .map(|value| (mode, value)),
                    ),
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        work
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum FactoryMode {
    Instance,
    Constructor,
}

impl FactoryMode {
    fn depth_limit(self) -> u32 {
        match self {
            Self::Instance => 6,
            Self::Constructor => 8,
        }
    }
}

#[cfg(test)]
#[path = "factory_return_tests.rs"]
mod factory_return_tests;

struct FactoryCx<'a> {
    fn_of_local: &'a FxHashMap<u32, u32>,
    init_expr: &'a FxHashMap<u32, Expr>,
    class_of_local: &'a std::collections::HashMap<u32, String>,
}

impl Census {
    fn access(&self, this_class: Option<u32>, object: &Expr, key: &str, store: bool) {
        match object {
            Expr::LocalGet(id) => {
                note(self.locals.borrow_mut().entry(*id).or_default(), key, store)
            }
            Expr::This => {
                if let Some(c) = this_class {
                    note(self.thises.borrow_mut().entry(c).or_default(), key, store)
                }
            }
            _ => {}
        }
    }

    fn note_returns(&self, func: u32, body: &[Stmt]) {
        for s in body {
            visit_stmt(
                s,
                &mut |st| {
                    if let Stmt::Return(Some(e)) = st {
                        if factory_form(e) {
                            self.factories
                                .borrow_mut()
                                .returns
                                .entry(func)
                                .or_default()
                                .push(e.clone());
                        }
                    }
                    if let Stmt::Return(Some(Expr::LocalGet(id))) = st {
                        self.returns
                            .borrow_mut()
                            .entry(func)
                            .and_modify(|r| {
                                if *r != Some(*id) {
                                    *r = None
                                }
                            })
                            .or_insert(Some(*id));
                    }
                },
                &mut |_| {},
            );
        }
    }

    fn body(&self, body: &[Stmt], this_class: Option<u32>, func: Option<u32>) {
        if let Some(f) = func {
            self.note_returns(f, body);
        }
        visit_stmts(
            body,
            &mut |s| {
                if let Stmt::Let {
                    id,
                    init: Some(init),
                    ..
                } = s
                {
                    if factory_form(init) {
                        self.factories
                            .borrow_mut()
                            .assigns
                            .entry(*id)
                            .or_default()
                            .push(init.clone());
                    }
                    // A call or a `new` names the classes the local's object
                    // is a fresh instance of (`Factories::instance_classes`).
                    if let Expr::Call { .. } | Expr::New { .. } = init {
                        self.init_expr.borrow_mut().insert(*id, init.clone());
                    }
                    match init {
                        Expr::Closure { func_id, .. } => {
                            self.fn_of_local.borrow_mut().insert(*id, *func_id);
                        }
                        Expr::FuncRef(f) => {
                            self.fn_of_local.borrow_mut().insert(*id, *f);
                        }
                        Expr::Call { callee, args, .. } => {
                            self.init_call.borrow_mut().insert(*id, (**callee).clone());
                            self.init_literals
                                .borrow_mut()
                                .insert(*id, args.iter().map(number_literal).collect());
                        }
                        Expr::New { args, .. } => {
                            self.born.borrow_mut().insert(*id);
                            self.init_literals
                                .borrow_mut()
                                .insert(*id, args.iter().map(number_literal).collect());
                        }
                        _ => {}
                    }
                }
            },
            &mut |e| match e {
                Expr::PropertyGet {
                    object, property, ..
                } => self.access(this_class, object, property, false),
                Expr::PropertySet {
                    object, property, ..
                }
                | Expr::PropertyUpdate {
                    object, property, ..
                } => self.access(this_class, object, property, true),
                Expr::PutValueSet {
                    target,
                    key,
                    receiver,
                    ..
                } if std::mem::discriminant(target.as_ref())
                    == std::mem::discriminant(receiver.as_ref()) =>
                {
                    if let Expr::String(k) = key.as_ref() {
                        self.access(this_class, target, k, true)
                    }
                }
                Expr::Closure { func_id, body, .. } => self.note_returns(*func_id, body),
                Expr::LocalSet(id, v) if factory_form(v) => {
                    self.factories
                        .borrow_mut()
                        .assigns
                        .entry(*id)
                        .or_default()
                        .push((**v).clone());
                }
                Expr::Object(_) | Expr::New { .. } => {
                    // A closed-shape literal lowers to `new __AnonShape_<hash>(values)`,
                    // its keys the synthesized class's fields in order.
                    let props: Vec<(&String, &Expr)> = match e {
                        Expr::Object(props) => props.iter().map(|(k, v)| (k, v)).collect(),
                        Expr::New {
                            class_name, args, ..
                        } => match self.anon_fields.get(class_name) {
                            Some(fields) if fields.len() == args.len() => {
                                fields.iter().zip(args.iter()).collect()
                            }
                            _ => Vec::new(),
                        },
                        _ => Vec::new(),
                    };
                    let mut f = self.factories.borrow_mut();
                    for (name, v) in props {
                        let func = match v {
                            Expr::Closure { func_id, .. } => Some(*func_id),
                            Expr::FuncRef(id) => Some(*id),
                            Expr::LocalGet(l) => {
                                f.named_locals.entry(name.clone()).or_default().push(*l);
                                None
                            }
                            _ => None,
                        };
                        if let Some(func) = func {
                            f.named.entry(name.clone()).or_default().push(func);
                        }
                    }
                }
                _ => {}
            },
        );
    }
}

/// The receiver groups of one module: (`this` class or none, group).
pub(super) fn module_groups(hir: &HirModule) -> Vec<(Option<u32>, Group)> {
    let c = Census {
        anon_fields: hir
            .classes
            .iter()
            .filter(|c| c.name.starts_with("__AnonShape_"))
            .map(|c| {
                (
                    c.name.clone(),
                    c.fields.iter().map(|f| f.name.clone()).collect(),
                )
            })
            .collect(),
        ..Default::default()
    };
    for f in &hir.functions {
        c.factories
            .borrow_mut()
            .named
            .entry(f.name.clone())
            .or_default()
            .push(f.id);
    }
    c.body(&hir.init, None, None);
    for f in &hir.functions {
        c.body(&f.body, None, Some(f.id));
    }
    for class in &hir.classes {
        for f in class.constructor.iter().chain(class.methods.iter()) {
            c.body(&f.body, Some(class.id), None);
        }
        for (_, f) in class.getters.iter().chain(class.setters.iter()) {
            c.body(&f.body, Some(class.id), None);
        }
        for f in &class.static_methods {
            c.body(&f.body, None, None);
        }
    }
    let mut locals = c.locals.into_inner();
    let fn_of_local = c.fn_of_local.into_inner();
    let init_call = c.init_call.into_inner();
    let returns = c.returns.into_inner();
    let born = c.born.into_inner();
    let init_literals = c.init_literals.into_inner();
    let init_expr = c.init_expr.into_inner();
    let factories = c.factories.into_inner();
    let class_of_local = super::class_bindings(hir);
    let class_id_of: FxHashMap<&str, u32> = {
        let mut m: FxHashMap<&str, Option<u32>> = FxHashMap::default();
        for cl in &hir.classes {
            m.entry(cl.name.as_str())
                .and_modify(|v| *v = None)
                .or_insert(Some(cl.id));
        }
        m.into_iter().filter_map(|(k, v)| Some((k, v?))).collect()
    };
    // The group a local's initialising call returns.
    let callee_group = |id: u32| -> Option<u32> {
        let f = match init_call.get(&id)? {
            Expr::FuncRef(f) => *f,
            Expr::LocalGet(l) => *fn_of_local.get(l)?,
            _ => return None,
        };
        (*returns.get(&f)?).filter(|h| *h != id)
    };
    // Prefix each group with its callee groups' stores (a few levels deep).
    let mut prefixed: Vec<(u32, Vec<String>, FxHashMap<String, u32>)> = Vec::new();
    for id in locals.keys().copied() {
        let mut chain = vec![id];
        let mut cur = id;
        while let Some(h) = callee_group(cur) {
            if chain.len() > 8 || chain.contains(&h) {
                break;
            }
            chain.push(h);
            cur = h;
        }
        if chain.len() == 1 {
            continue;
        }
        let mut stores: Vec<String> = Vec::new();
        let mut sites: FxHashMap<String, u32> = FxHashMap::default();
        for g in chain.iter().rev() {
            if let Some(g) = locals.get(g) {
                for k in &g.stores {
                    if !stores.contains(k) {
                        stores.push(k.clone());
                    }
                }
                if !std::ptr::eq(g, &locals[&id]) {
                    continue;
                }
                for (k, n) in &g.sites {
                    *sites.entry(k.clone()).or_default() += n;
                }
            }
        }
        // A callee's stores count once per caller site.
        for k in &stores {
            sites.entry(k.clone()).or_insert(1);
        }
        prefixed.push((id, stores, sites));
    }
    for (id, stores, sites) in prefixed {
        if let Some(g) = locals.get_mut(&id) {
            g.keys.extend(stores.iter().cloned());
            g.stores = stores;
            g.sites = sites;
            g.creation = true;
        }
    }
    for id in born {
        if let Some(g) = locals.get_mut(&id) {
            g.creation = true;
        }
    }
    // A local initialised by a factory call whose result is a fresh
    // instance of known classes is a creation group of their families.
    let cx = FactoryCx {
        fn_of_local: &fn_of_local,
        init_expr: &init_expr,
        class_of_local: &class_of_local,
    };
    for (id, init) in &init_expr {
        let Some(g) = locals.get_mut(id) else {
            continue;
        };
        let mut names = BTreeSet::new();
        factories.instance_classes(init, &cx, 0, &mut names);
        let classes: BTreeSet<u32> = names
            .iter()
            .filter_map(|n| class_id_of.get(n.as_str()).copied())
            .collect();
        if !classes.is_empty() {
            g.creation = true;
            g.classes = classes;
        }
    }
    for (id, lits) in init_literals {
        if let Some(g) = locals.get_mut(&id) {
            if g.creation {
                g.init_literals = lits;
            }
        }
    }
    let mut out: Vec<(Option<u32>, Group)> = locals
        .into_iter()
        .map(|(id, mut g)| {
            g.local = Some(id);
            (None, g)
        })
        .collect();
    out.extend(c.thises.into_inner().into_iter().map(|(c, g)| (Some(c), g)));
    out
}

/// The families (indices into `owns`, their constructor keys) a local
/// group may hold objects of. The evidence is the group's constructor-key
/// usage `u` (its keys that are some family's constructor key):
///
/// * the family explains as much of `u` as any family does (coverage, ties
///   included): a receiver of another family that merely shares a common
///   name with this one (`flags`, `id`, `parent`: tsc's Type and Symbol
///   objects against Node) touches keys this family does not have;
/// * a group that touches one constructor key only counts when every family
///   with that key is alike (pairwise, at least half their constructor keys
///   shared): `kind` (Node, Token, Identifier, SourceFile) counts, `flags`
///   (Node, Symbol, Type) does not. `PERRY_LAYOUT_LATE_EVIDENCE=2` requires
///   two shared keys instead (measurement).
fn group_families(g: &Group, owns: &[BTreeSet<String>], class_ids: &[u32]) -> BTreeSet<usize> {
    // A factory-created local: the families of the classes it is a fresh
    // instance of ([`Factories`]).
    if !g.classes.is_empty() {
        return (0..class_ids.len())
            .filter(|&i| g.classes.contains(&class_ids[i]))
            .collect();
    }
    let u: Vec<&String> = g
        .keys
        .iter()
        .filter(|k| owns.iter().any(|o| o.contains(*k)))
        .collect();
    if u.is_empty() || u.len() < late_evidence() {
        return BTreeSet::new();
    }
    let common: Vec<usize> = owns
        .iter()
        .map(|o| u.iter().filter(|k| o.contains(**k)).count())
        .collect();
    let best = common.iter().copied().max().unwrap_or(0);
    let fams: BTreeSet<usize> = (0..owns.len()).filter(|&i| common[i] == best).collect();
    if u.len() == 1 {
        let alike = |a: &BTreeSet<String>, b: &BTreeSet<String>| {
            let shared = a.intersection(b).count();
            shared * 2 >= a.len().max(b.len())
        };
        let with: Vec<&BTreeSet<String>> = owns.iter().filter(|o| o.contains(u[0])).collect();
        if !with.iter().all(|a| with.iter().all(|b| alike(a, b))) {
            return BTreeSet::new();
        }
    }
    fams
}

/// Shared constructor keys a local group needs to be counted for a family
/// (`PERRY_LAYOUT_LATE_EVIDENCE` overrides it for measurement).
fn late_evidence() -> usize {
    std::env::var("PERRY_LAYOUT_LATE_EVIDENCE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}

/// The property names among a family's keys (late keys are properties: a
/// creation group stores names).
fn property_keys(keys: &[FamilyKey]) -> BTreeSet<String> {
    keys.iter()
        .filter_map(FamilyKey::name)
        .map(str::to_string)
        .collect()
}

/// Rank keys: the keys a creation group stores first (a key written while
/// the object is built precedes one a later phase adds: tsc's `escapedText`
/// before the binder's `symbol`), then by pairwise precedence over the
/// groups' store orders (Copeland: wins minus losses), ties by store sites,
/// then name.
fn late_order(
    keys: &[String],
    orders: &[&[String]],
    sites: &FxHashMap<String, u32>,
    creation: &BTreeSet<String>,
) -> Vec<String> {
    let pos: FxHashMap<&str, usize> = keys
        .iter()
        .enumerate()
        .map(|(i, k)| (k.as_str(), i))
        .collect();
    let n = keys.len();
    let mut before = vec![vec![0u32; n]; n];
    for o in orders {
        let idx: Vec<usize> = o
            .iter()
            .filter_map(|k| pos.get(k.as_str()).copied())
            .collect();
        for i in 0..idx.len() {
            for j in i + 1..idx.len() {
                before[idx[i]][idx[j]] += 1;
            }
        }
    }
    let mut score: Vec<(bool, i64, u32, &String)> = (0..n)
        .map(|i| {
            let s: i64 = (0..n)
                .map(|j| {
                    i64::from(before[i][j] > before[j][i]) - i64::from(before[j][i] > before[i][j])
                })
                .sum();
            (
                creation.contains(&keys[i]),
                s,
                sites.get(&keys[i]).copied().unwrap_or(0),
                &keys[i],
            )
        })
        .collect();
    score.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(b.1.cmp(&a.1))
            .then(b.2.cmp(&a.2))
            .then(a.3.cmp(b.3))
    });
    score.into_iter().map(|(_, _, _, k)| k.clone()).collect()
}

/// Layouts in one family's creation split, at most.
const MAX_VARIANTS: usize = 256;
/// Keys a variant takes from its creation sites, at most.
const VARIANT_CREATION_BUDGET: usize = 32;

/// Is the creation split off (`PERRY_LAYOUT_VARIANTS=0`, measurement)?
fn variants_disabled() -> bool {
    std::env::var("PERRY_LAYOUT_VARIANTS").as_deref() == Ok("0")
}

/// The creation split (§3.2: a family is a creation point). A root family
/// whose constructor stores parameter `p` into a constructor key (tsc's
/// `NodeObject(kind, pos, end)`: `this.kind = kind`) is created through
/// factories that pass a constant for it (`createBaseDeclaration(211)`):
/// the creation groups that pass constant `K` define the layout of the
/// births whose argument `p` is `K`. Each such variant holds the
/// constructor keys, then the keys its creation groups store (in their
/// order), then the keys later phases store on the family (the groups that
/// are not creations: tsc's binder, its `jsDoc` attachment). The parameter
/// chosen is the one with the most distinct constants; a family with fewer
/// than two keeps one layout. Only the birth reads the argument, and every
/// variant is a correct layout of every instance (its keys are holes until
/// stored): the split decides width and slots, never semantics.
pub(super) fn add_variants(raw: &mut [RawFamily], groups: &[(Option<u32>, Group)]) {
    let budget = late_budget();
    if budget == 0 || variants_disabled() {
        return;
    }
    let owns: Vec<BTreeSet<String>> = raw.iter().map(|f| property_keys(&f.keys)).collect();
    let class_ids: Vec<u32> = raw.iter().map(|f| f.class_id).collect();
    let group_fams: Vec<BTreeSet<usize>> = groups
        .iter()
        .map(|(this_class, g)| {
            if this_class.is_some() {
                BTreeSet::new()
            } else {
                group_families(g, &owns, &class_ids)
            }
        })
        .collect();
    for i in 0..raw.len() {
        if raw[i].params.is_empty() {
            if explain() && raw[i].chain.len() == 1 {
                eprintln!(
                    "perry: layout family split: class {} no parameter-stored constructor key",
                    raw[i].class_id
                );
            }
            continue;
        }
        let f = &raw[i];
        let usable = |k: &String| !owns[i].contains(k) && !f.members.contains(k) && late_key_ok(k);
        // The family's creation groups, and the parameter that splits them.
        let creations: Vec<&Group> = groups
            .iter()
            .enumerate()
            .filter(|(gi, (this_class, g))| {
                this_class.is_none() && g.creation && group_fams[*gi].contains(&i)
            })
            .map(|(_, (_, g))| g)
            .collect();
        let mut best: Option<(u32, BTreeMap<u64, Vec<&Group>>)> = None;
        for (p, _) in &f.params {
            let mut by: BTreeMap<u64, Vec<&Group>> = BTreeMap::new();
            for g in &creations {
                if let Some(Some(k)) = g.init_literals.get(*p as usize) {
                    by.entry(k.to_bits()).or_default().push(g);
                }
            }
            if by.len() >= 2 && best.as_ref().is_none_or(|(_, b)| by.len() > b.len()) {
                best = Some((*p, by));
            }
        }
        if explain() {
            let lits = creations
                .iter()
                .filter(|g| g.init_literals.iter().any(Option::is_some))
                .count();
            eprintln!(
                "perry: layout family split: class {} params {:?} creation groups {} with literals {} -> {}",
                f.class_id,
                f.params,
                creations.len(),
                lits,
                best.as_ref().map_or(0, |(_, b)| b.len())
            );
        }
        let Some((param, by)) = best else {
            continue;
        };
        // The later phases' keys: groups that may hold the family's objects
        // but do not create them, and the methods of its chain.
        let mut phase_sites: FxHashMap<String, u32> = FxHashMap::default();
        let mut phase_orders: Vec<&[String]> = Vec::new();
        for (gi, (this_class, g)) in groups.iter().enumerate() {
            let may = match this_class {
                Some(c) => f.chain.contains(c),
                None => !g.creation && group_fams[gi].contains(&i),
            };
            if !may {
                continue;
            }
            phase_orders.push(&g.stores);
            for (k, n) in &g.sites {
                if usable(k) {
                    *phase_sites.entry(k.clone()).or_default() += n;
                }
            }
        }
        let mut phase: Vec<(&String, &u32)> = phase_sites.iter().collect();
        phase.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let phase: Vec<String> = phase
            .into_iter()
            .take(budget)
            .map(|(k, _)| k.clone())
            .collect();
        let phase = late_order(&phase, &phase_orders, &phase_sites, &BTreeSet::new());
        // Most-created constants first, up to the bound.
        let mut split: Vec<(u64, Vec<&Group>)> = by.into_iter().collect();
        split.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
        let mut variants = Vec::new();
        for (k, gs) in split.into_iter().take(MAX_VARIANTS) {
            let mut sites: FxHashMap<String, u32> = FxHashMap::default();
            let mut orders: Vec<&[String]> = Vec::new();
            for g in &gs {
                orders.push(&g.stores);
                for (key, n) in &g.sites {
                    if usable(key) {
                        *sites.entry(key.clone()).or_default() += n;
                    }
                }
            }
            let mut cand: Vec<(&String, &u32)> = sites.iter().collect();
            cand.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            let cand: Vec<String> = cand
                .into_iter()
                .take(VARIANT_CREATION_BUDGET)
                .map(|(k, _)| k.clone())
                .collect();
            let all: BTreeSet<String> = cand.iter().cloned().collect();
            let mut keys = f.keys.clone();
            keys.extend(
                late_order(&cand, &orders, &sites, &all)
                    .into_iter()
                    .map(FamilyKey::Name),
            );
            for k in &phase {
                if !keys.iter().any(|x| x == k) {
                    keys.push(FamilyKey::Name(k.clone()));
                }
            }
            variants.push((f64::from_bits(k), keys));
        }
        raw[i].discriminant = param;
        raw[i].variants = variants;
    }
}

/// Append every family's late keys to its keys (parents first: a family
/// keeps its parent's late keys, then adds its own up to the budget).
pub(super) fn add_late_keys(raw: &mut [RawFamily], groups: &[(Option<u32>, Group)]) {
    let budget = late_budget();
    // Every family's constructor keys, before any late key joins them.
    let owns: Vec<BTreeSet<String>> = raw.iter().map(|f| property_keys(&f.keys)).collect();
    let class_ids: Vec<u32> = raw.iter().map(|f| f.class_id).collect();
    // Per local group, the families it may hold objects of.
    let group_fams: Vec<BTreeSet<usize>> = groups
        .iter()
        .map(|(this_class, g)| {
            if this_class.is_some() {
                BTreeSet::new()
            } else {
                group_families(g, &owns, &class_ids)
            }
        })
        .collect();
    let mut order: Vec<usize> = (0..raw.len()).collect();
    order.sort_by_key(|&i| raw[i].chain.len());
    let mut late_of: FxHashMap<u32, Vec<String>> = FxHashMap::default();
    for i in order {
        let f = &raw[i];
        let own: BTreeSet<&str> = f.keys.iter().filter_map(FamilyKey::name).collect();
        // The parent's late keys, except a name this class's chain declares
        // as a member (`IdentifierObject`'s `get text()` over its parent's
        // late `text`): a store of it meets the accessor, so no slot holds it,
        // and a constructor run whose chain holds one is refused by the run
        // check (`perry-runtime` `perry_family_run_admits`).
        let inherited: Vec<String> = late_of
            .get(&f.parent)
            .map(|v| {
                v.iter()
                    .filter(|k| !own.contains(k.as_str()) && !f.members.contains(*k))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let mut sites: FxHashMap<String, u32> = FxHashMap::default();
        let mut orders: Vec<&[String]> = Vec::new();
        let mut creation: BTreeSet<String> = BTreeSet::new();
        // The creation groups known to create this family's objects (a
        // factory resolved to its class): their store order is the
        // instances' insertion order, so their keys lead the key sequence.
        let mut exact_orders: Vec<&[String]> = Vec::new();
        let mut exact: BTreeSet<String> = BTreeSet::new();
        for (gi, (this_class, g)) in groups.iter().enumerate() {
            let may = match this_class {
                Some(c) => f.chain.contains(c),
                None => group_fams[gi].contains(&i),
            };
            if !may {
                continue;
            }
            if g.creation && g.classes.contains(&f.class_id) {
                exact_orders.push(&g.stores);
                exact.extend(g.stores.iter().cloned());
            }
            orders.push(&g.stores);
            if g.creation {
                creation.extend(g.stores.iter().cloned());
            }
            for (k, n) in &g.sites {
                if !own.contains(k.as_str()) && !f.members.contains(k) && late_key_ok(k) {
                    *sites.entry(k.clone()).or_default() += n;
                }
            }
        }
        // A parent's late key this family's own sites never store is width
        // only (tsc's TokenOrIdentifierObject keys on IdentifierObject).
        let inherited: Vec<String> = inherited
            .into_iter()
            .filter(|k| sites.contains_key(k))
            .collect();
        let mut cand: Vec<(&String, &u32)> = sites
            .iter()
            .filter(|(k, _)| !inherited.contains(k))
            .collect();
        cand.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        // The budget is the family's own: the parent's late keys keep their
        // slots and do not crowd out the keys this family's own creation
        // sites store (tsc's IdentifierObject `escapedText` under its
        // parent's eight).
        let room = budget;
        let chosen: Vec<String> = cand
            .into_iter()
            .take(room)
            .map(|(k, _)| k.clone())
            .collect();
        // The parent's late keys keep its slots (`assign_slots`), not its
        // order: the family ranks them with its own.
        let mut ranked = inherited;
        ranked.extend(chosen);
        let (first, rest): (Vec<String>, Vec<String>) =
            ranked.into_iter().partition(|k| exact.contains(k));
        let mut late = late_order(&first, &exact_orders, &sites, &exact);
        late.extend(late_order(&rest, &orders, &sites, &creation));
        if explain() && !late.is_empty() {
            eprintln!(
                "perry: layout family late keys: class {} (+{}): {}",
                f.class_id,
                late.len(),
                late.join(",")
            );
        }
        late_of.insert(f.class_id, late.clone());
        raw[i].keys.extend(late.into_iter().map(FamilyKey::Name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn group(keys: &[&str]) -> Group {
        Group {
            keys: set(keys),
            ..Default::default()
        }
    }

    #[test]
    fn a_shared_common_name_does_not_pull_in_another_family() {
        // Node, Type, Symbol and Token constructor keys (tsc's shapes).
        let owns = vec![
            set(&["pos", "end", "kind", "id", "flags", "parent", "original"]),
            set(&["flags", "checker"]),
            set(&["flags", "escapedName", "id", "parent"]),
            set(&["pos", "end", "kind", "id", "flags", "parent"]),
        ];
        let fams = |keys: &[&str]| group_families(&group(keys), &owns, &[0, 1, 2, 3]);
        // A Type receiver: only Type explains `flags` and `checker`.
        assert_eq!(fams(&["flags", "checker", "objectFlags"]), [1].into());
        // A Symbol receiver.
        assert_eq!(fams(&["flags", "escapedName", "declarations"]), [2].into());
        // `flags` alone names unlike families: no family.
        assert!(fams(&["flags", "symbol"]).is_empty());
        // `kind` alone names the Node-like families only.
        assert_eq!(fams(&["kind", "symbol"]), [0, 3].into());
        // A Node receiver that also reads `original` is a Node.
        assert_eq!(fams(&["kind", "original", "flowNode"]), [0].into());
        // No constructor key: no family.
        assert!(fams(&["symbol", "flowNode"]).is_empty());
    }
}

/// Select one unambiguous literal family by the existing creation/use census.
/// This is not a lifetime proof: the read always checks the current ShapeId.
pub(super) fn receiver_candidates(
    hir: &HirModule,
    layouts: &super::ProgramLayouts,
) -> BTreeMap<u32, u32> {
    if layouts.literal_families.is_empty() {
        return BTreeMap::new();
    }
    let families: Vec<_> = layouts.families.values().collect();
    let owns: Vec<_> = families
        .iter()
        .map(|f| {
            property_keys(
                &f.keys[..f.own_count as usize]
                    .iter()
                    .map(|(key, _)| key.clone())
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let ids: Vec<_> = families.iter().map(|f| f.class_id).collect();
    module_groups(hir)
        .into_iter()
        .filter_map(|(this_class, group)| {
            if this_class.is_some() {
                return None;
            }
            let id = group.local?;
            let candidates = group_families(&group, &owns, &ids);
            if candidates.len() != 1 {
                return None;
            }
            let class = ids[*candidates.first()?];
            layouts
                .literal_families
                .contains(&class)
                .then_some((id, class))
        })
        .collect()
}
