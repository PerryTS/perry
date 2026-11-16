//! Declaration units inside a module (design §12.1, §12.6 steps 1-2): the
//! module scope's declarations and statements, joined along references,
//! cut into the SCCs of that graph. A unit is solved over a view of the
//! module: its own items, every other unit's declaration entering as an
//! import of that unit, exactly as another module's export does (§11).

use crate::pts::{FxHashMap, FxHashSet};
use perry_hir::tdz_check::{for_each_class_expr, for_each_expr_in, for_each_function_expr};
use perry_hir::{Export, Expr, Import, Module, Stmt};

/// A declaration unit's view of its module: the HIR ids of the other
/// units' declarations it refers to, each bound to the import named by the
/// declaration (module-scope names are unique: lowering keeps one function
/// and one class per name), and its own declarations other units import.
#[derive(Clone, Debug, Default)]
pub(crate) struct UnitView<'a> {
    pub closure_bodies: FxHashMap<u32, &'a Expr>,
    /// Bindings declared by this unit's source items. Literal-layout classes
    /// copied into the view are analysis context, not owned lexical bodies.
    pub binding_locals: FxHashSet<u32>,
    pub binding_funcs: FxHashSet<u32>,
    pub foreign_locals: FxHashMap<u32, String>,
    pub foreign_funcs: FxHashMap<u32, String>,
    pub foreign_globals: FxHashMap<u32, String>,
    /// Names exported only to the other units of the module (§12.3): the
    /// cut's own plumbing, not exports of the module, so the module's
    /// namespace object does not hold them.
    pub unit_exports: FxHashSet<String>,
    /// The unit's key (`unit_path`): a cell it reads joins the other units'
    /// writes, not its own (design §17.16 (b)).
    pub key: String,
    /// Its closed functions (decision 13, [`closed_bindings`]): name, HIR
    /// id of the body.
    pub closed: FxHashMap<String, u32>,
    /// The other units' closed functions it refers to (it calls them).
    pub closed_refs: Vec<String>,
    /// DECISIONS 157: the published classes among `closed` (exported, used
    /// as a value, extended, with static members): every unit holding one
    /// holds its stand-in, which forwards it as an open class's and
    /// publishes what its constructions pass.
    pub published: FxHashSet<String>,
    /// DECISIONS 195: its closed locals (decision 23) passed by name as a
    /// direct argument: the only ones whose value can reach another unit's
    /// code. Any other closed local is called only here.
    pub closed_passed: FxHashSet<String>,
    /// Its index among its module's units ([`unit_path`]).
    pub unit: usize,
    /// The other units' module-scope bindings it writes (`currentFlow = f`,
    /// `n++`), by HIR id with the local name of their import: written
    /// through the binding's write effect (`Gen::local_write`), not joined.
    pub written: Vec<(u32, String)>,
    /// The names it refers to that no unit of its module owns: its imports
    /// (and globals). The driver orders it after the units owning them
    /// (decision 21).
    pub ext_names: FxHashSet<String>,
    /// The `ext_names` that only parameter types name (§11.4): the entry guard
    /// reads their contracts, no value of theirs is used.
    pub type_names: FxHashSet<String>,
    /// The units of this module only its parameter types name.
    pub type_deps: Vec<usize>,
    /// The member paths read off imported names (`ns.k`, `ns.a.b`): for a
    /// namespace import, the keys whose declaring units it reads.
    pub ns_paths: Vec<(String, Vec<String>)>,
    /// Its items, as indices into the one module it is cut from: a view
    /// borrows the module, it copies nothing of it (phase 03m, CWN3M: the
    /// per-unit module clones were 16.4 GB of tscwork's 23 GB). `classes`
    /// holds its own classes, then the literal shapes it births (one class
    /// seen by every unit that names it, under the shared class id rule).
    pub funcs: Vec<usize>,
    pub classes: Vec<usize>,
    pub globals: Vec<usize>,
    pub init: Vec<usize>,
    /// The synthetic imports of the other units' names (`unit_path`), read
    /// after the module's own imports.
    pub imports: Vec<Import>,
    /// Its exports: the module's own that it owns (re-exports in unit 0)
    /// and the names other units import, as `Export::Named`.
    pub exports: Vec<Export>,
    /// The module-wide item tables, its own entries only (each unit exports
    /// what it owns; the whole table in every unit made each unit publish
    /// ⊤ under every name).
    pub exported_objects: Vec<String>,
    pub exported_functions: Vec<(String, u32)>,
}

/// One item of the module scope.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Item {
    Func(usize),
    Class(usize),
    Global(usize),
    Init(usize),
}

/// The path of unit `k` of module `path`: the key its contracts are
/// published under.
pub(crate) fn unit_path(path: &str, k: usize) -> String {
    format!("{path}#u{k}")
}

/// The module path of a unit path (any other path: itself).
pub(crate) fn module_of(p: &str) -> &str {
    match p.rsplit_once("#u") {
        Some((m, k)) if !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()) => m,
        _ => p,
    }
}

/// What an item refers to: read edges (`to`) and joins (both ways: a
/// class's state, a local two items share), and the locals it writes.
#[derive(Default)]
struct Refs {
    names: Vec<String>,
    locals: Vec<u32>,
    funcs: Vec<u32>,
    globals: Vec<u32>,
    join_names: Vec<String>,
    join_locals: Vec<u32>,
    join_globals: Vec<u32>,
    /// The chunk functions of a literal birth it holds (a call passing
    /// `Expr::LiteralChunkObject`): joined, they initialize its object.
    join_funcs: Vec<u32>,
    /// Locals the item declares (closure parameters here; `declared` adds
    /// its functions' parameters, `let`s and catch bindings).
    declared: Vec<u32>,
    /// The closures inside the item.
    closures: Vec<u32>,
    /// The member paths read off external names (`ns.a.b`: `("ns", [a, b])`).
    ns_paths: Vec<(String, Vec<String>)>,
    /// The named types of its parameters (an entry guard's class, §11.4).
    param_types: Vec<String>,
    /// The locals it writes (also in `locals`).
    writes: Vec<u32>,
}





/// Expressions that name a class by its name.
fn class_name_refs(e: &Expr, r: &mut Refs) {
    match e {
        Expr::New { class_name, .. }
        | Expr::JsNew { class_name, .. }
        | Expr::StaticFieldGet { class_name, .. }
        | Expr::StaticMethodCall { class_name, .. }
        | Expr::ClassMethodHome { class_name, .. }
        | Expr::ClassEnvGet { class_name, .. }
        | Expr::ClassEnvCurrent { class_name, .. }
        | Expr::ClassCaptureValue { class_name, .. }
        | Expr::PrivateBrandCheck { class_name, .. }
        | Expr::PrivateGuard { class_name, .. }
        | Expr::EnumImage { class_name, .. } => r.names.push(class_name.clone()),
        Expr::NativeMethodCall {
            class_name: Some(class_name),
            ..
        }
        | Expr::SuperPropertySet {
            parent_class_name: Some(class_name),
            ..
        } => r.names.push(class_name.clone()),
        // A class expression's evaluation (`var _V = class _V {..}` in a
        // run-once body: the HIR's `ClassExprFresh`) is its class's
        // definition: it makes the class object and runs its static
        // initializers, the class's state, as the declaration's own static
        // stores are (`StaticFieldSet`). Not naming it left the evaluating
        // unit without the class (a ⊤ value from unknown code there) and
        // the class unit without the units constructing it.
        Expr::ClassExprFresh {
            template: class_name,
            ..
        } => r.join_names.push(class_name.clone()),
        // A refresh re-stores into its class's environment the current
        // values of the bindings the class's evaluation captured, slot by
        // slot (`captured_args`, the same capture list). The model holds one
        // value per binding over the whole run, which that evaluation already
        // stores into each slot: the refresh adds no state to the class. It
        // reads the class and its captures (the walk records those) and
        // writes nothing another unit holds. (tsc: a refresh after every
        // assignment to a captured binding of a `ts.server` class joined 667
        // module-scope statements to the classes.)
        Expr::RefreshClassExprCaptures {
            env_class: Some(class_name),
            ..
        } => r.names.push(class_name.clone()),
        Expr::StaticFieldSet { class_name, .. }
        | Expr::ClassStaticSymbolSet { class_name, .. }
        | Expr::ClassEnvSet { class_name, .. }
        | Expr::ClassEnvStamp { class_name, .. }
        | Expr::RegisterClassCaptures { class_name, .. }
        | Expr::RegisterClassComputedAccessor { class_name, .. }
        | Expr::RegisterClassComputedMethod { class_name, .. }
        | Expr::RegisterClassParentDynamic { class_name, .. }
        | Expr::RegisterClassStaticSymbol { class_name, .. }
        | Expr::RegisterPrototypeMethod { class_name, .. } => r.join_names.push(class_name.clone()),
        _ => {}
    }
}










/// The lowering of an object literal in source order (`((__perry_obj_iife)
/// => { …; return __perry_obj_iife })({})`): an expression, whose defines
/// are the literal's birth (`Gen::call_expr`), not a body that runs once.
/// Flat, its parts would be module-scope items of their own, the literal
/// born empty in one unit and its keys stored by others (design §18.9).
pub(crate) fn obj_literal_iife(params: &[perry_hir::Param]) -> bool {
    params.len() == 1 && params[0].name == "__perry_obj_iife"
}


/// Decision 23: every `let` an item declares inside its functions (any
/// depth, closure bodies included; class members not: their locals stay
/// unknown values), with the function literal it binds, if any, and its
/// closed key `<name>#<func id>` (locals of one name stay apart).
fn local_lets(module: &Module, it: Item, out: &mut Vec<(u32, Option<(String, u32)>)>) {
    use perry_hir::tdz_check::{for_each_expr_stmt_deep, for_each_stmt_deep};
    let mut on_stmt = |s: &Stmt| {
        if let Stmt::Let { id, name, init, .. } = s {
            let lit = match init {
                Some(Expr::Closure { func_id, .. }) => {
                    Some((format!("{name}#{func_id}"), *func_id))
                }
                _ => None,
            };
            out.push((*id, lit));
        }
    };
    match it {
        Item::Func(k) => for_each_stmt_deep(&module.functions[k].body, &mut on_stmt),
        Item::Global(k) => {
            if let Some(e) = &module.globals[k].init {
                for_each_expr_stmt_deep(e, &mut on_stmt);
            }
        }
        Item::Init(k) => for_each_stmt_deep(std::slice::from_ref(&module.init[k]), &mut on_stmt),
        Item::Class(_) => {}
    }
}



/// The closed bindings of `module` (decision 13, every export counting)
/// whose every reference is the callee of a direct call or a TDZ check:
/// none is passed as an argument, so only those calls run it. By key
/// (`(true, function id)`, `(false, local id)`), with the body's id.
pub(crate) fn call_only_bindings(module: &Module) -> FxHashMap<(bool, u32), u32> {
    let (cand, _, _, passed) = closed(module, None, &|_| true);
    cand.into_iter()
        .filter(|(_, (name, _))| !passed.contains(name))
        .map(|(k, (_, body))| (k, body))
        .collect()
}

/// The closed bindings by key (`(true, function id)` for a module function,
/// `(false, local id)` for a `let` bound to a function literal), each with
/// its name (or closed key) and body id.
type Closed = FxHashMap<(bool, u32), (String, u32)>;

/// The module's closed bindings by key, and every closed name with its
/// body's id (classes too), with the closed classes some unit publishes
/// and the closed ones passed by name as a direct argument
/// (`closed_bindings`). Without `unit_of` every member access counts as a
/// use that opens the binding (run-once flattening, `run_once_flat`, as
/// before DECISIONS 93); `named` says which exports another module names
/// (DECISIONS 129: every one for the flattening).
fn closed(
    module: &Module,
    unit_of: Option<&dyn Fn(Item) -> usize>,
    named: &dyn Fn(&str) -> bool,
) -> (
    Closed,
    FxHashMap<String, u32>,
    FxHashSet<String>,
    FxHashSet<String>,
) {
    #[derive(Default)]
    struct Use {
        refs: u32,
        calls: u32,
        written: bool,
        /// The callees it is passed to as a direct argument (decision 19).
        args_of: Vec<(bool, u32)>,
        /// TDZ checks of the binding (`tdz_check::check`): they read the
        /// slot for the dead-zone hole and yield no value of it.
        checks: u32,
        /// Member accesses on it (DECISIONS 93), the items they are in, and
        /// the method calls among them (the receiver is a value use).
        members: u32,
        member_items: Vec<Item>,
        receivers: u32,
    }
    let mut uses: FxHashMap<(bool, u32), Use> = FxHashMap::default();
    // Per class name: the expressions naming it, and the `new`s of it.
    let mut class_uses: FxHashMap<String, (u32, u32)> = FxHashMap::default();
    let mut with = false;
    {
        let mut named = |e: &Expr| {
            let mut r = Refs::default();
            class_name_refs(e, &mut r);
            if let Expr::ClassRef(n) = e {
                r.names.push(n.clone());
            }
            for n in r.names.into_iter().chain(r.join_names) {
                class_uses.entry(n).or_default().0 += 1;
            }
            if let Expr::New { class_name, .. } = e {
                class_uses.entry(class_name.clone()).or_default().1 += 1;
            }
        };
        let key = |e: &Expr| match e {
            Expr::LocalGet(id) => Some((false, *id)),
            Expr::FuncRef(id) => Some((true, *id)),
            _ => None,
        };
        let cur = std::cell::Cell::new(Item::Init(0));
        let f = &mut |e: &Expr| {
            named(e);
            let mut member = |o: &Expr, n: u32| {
                if let Some(k) = key(o) {
                    let u = uses.entry(k).or_default();
                    u.members += n;
                    u.member_items.push(cur.get());
                }
            };
            match e {
                Expr::PropertyGet { object, .. }
                | Expr::PropertySet { object, .. }
                | Expr::PropertyUpdate { object, .. }
                | Expr::IndexGet { object, .. }
                | Expr::IndexSet { object, .. }
                | Expr::IndexUpdate { object, .. } => member(object, 1),
                Expr::PutValueSet {
                    target, receiver, ..
                } if key(target).is_some() && key(target) == key(receiver) => member(target, 2),
                _ => {}
            }
            match e {
                Expr::Call { callee, args, .. } => {
                    if let Some(id) = perry_hir::tdz_check::checked_binding(e) {
                        uses.entry((false, id)).or_default().checks += 1;
                    }
                    if let Expr::PropertyGet { object, .. } | Expr::IndexGet { object, .. } =
                        &**callee
                    {
                        if let Some(k) = key(object) {
                            uses.entry(k).or_default().receivers += 1;
                        }
                    }
                    if let Some(k) = key(callee) {
                        uses.entry(k).or_default().calls += 1;
                        for a in args.iter().filter_map(key) {
                            uses.entry(a).or_default().args_of.push(k);
                        }
                    }
                }
                Expr::LocalGet(id) => uses.entry((false, *id)).or_default().refs += 1,
                Expr::FuncRef(id) => uses.entry((true, *id)).or_default().refs += 1,
                Expr::LocalSet(id, _)
                | Expr::Update { id, .. }
                | Expr::ScopedTemp { id, .. }
                | Expr::ArrayPush { array_id: id, .. }
                | Expr::ArrayPushSpread { array_id: id, .. }
                | Expr::ArrayUnshift { array_id: id, .. }
                | Expr::ArraySplice { array_id: id, .. }
                | Expr::ArrayCopyWithin { array_id: id, .. }
                | Expr::ArrayPop(id)
                | Expr::ArrayShift(id)
                | Expr::SetAdd { set_id: id, .. } => {
                    uses.entry((false, *id)).or_default().written = true
                }
                Expr::WithGet { .. } | Expr::WithSet { .. } => with = true,
                _ => {}
            }
        };
        for (k, func) in module.functions.iter().enumerate() {
            cur.set(Item::Func(k));
            for_each_function_expr(func, f);
        }
        for (k, c) in module.classes.iter().enumerate() {
            cur.set(Item::Class(k));
            for_each_class_expr(c, f);
        }
        for (k, g) in module.globals.iter().enumerate() {
            cur.set(Item::Global(k));
            if let Some(e) = &g.init {
                for_each_expr_in(e, f);
            }
        }
        for (k, s) in module.init.iter().enumerate() {
            cur.set(Item::Init(k));
            perry_hir::tdz_check::for_each_stmts_expr(std::slice::from_ref(s), f);
        }
    }
    if with {
        return Default::default();
    }
    let mut names: FxHashMap<&str, u32> = FxHashMap::default();
    // Per local: its module-scope `let`s besides the function literal one;
    // a `var`'s hoisted placeholder (no value, before it) is the binding
    // still holding undefined, no other value.
    let mut lets: FxHashMap<u32, Vec<(usize, bool)>> = FxHashMap::default();
    for n in module
        .functions
        .iter()
        .map(|f| f.name.as_str())
        .chain(module.classes.iter().map(|c| c.name.as_str()))
    {
        *names.entry(n).or_default() += 1;
    }
    for n in module.globals.iter().map(|g| g.name.as_str()) {
        *names.entry(n).or_default() += 1;
    }
    for (k, s) in module.init.iter().enumerate() {
        if let Stmt::Let { id, name, init, .. } = s {
            let placeholder = matches!(init, None | Some(Expr::Undefined));
            // One binding per id: a name counts once for its statements.
            if !lets.contains_key(id) {
                *names.entry(name.as_str()).or_default() += 1;
            }
            lets.entry(*id).or_default().push((k, placeholder));
        }
    }
    let exported = |name: &str| {
        module.exports.iter().any(
            |e| matches!(e, Export::Named { local, exported } if local == name && named(exported)),
        ) || module
            .exported_objects
            .iter()
            .any(|n| n == name && named(n))
    };
    // The candidates: unexported, uniquely named, never written.
    let mut cand: Closed = FxHashMap::default();
    // The item declaring a candidate (DECISIONS 93); none for a local `let`.
    let mut decl: FxHashMap<(bool, u32), Item> = FxHashMap::default();
    let unwritten = |u: Option<&Use>| u.is_none_or(|u| !u.written);
    for (fk, func) in module.functions.iter().enumerate() {
        let named = |l: &[(String, u32)]| l.iter().any(|(_, id)| *id == func.id);
        if names[func.name.as_str()] == 1
            && !exported(&func.name)
            && !named(&module.exported_functions)
            && !named(&module.script_global_functions)
            && unwritten(uses.get(&(true, func.id)))
        {
            cand.insert((true, func.id), (func.name.clone(), func.id));
            decl.insert((true, func.id), Item::Func(fk));
        }
    }
    for (k, s) in module.init.iter().enumerate() {
        if let Stmt::Let {
            id,
            name,
            init: Some(Expr::Closure { func_id, .. }),
            ..
        } = s
        {
            let once = lets[id]
                .iter()
                .all(|&(j, placeholder)| j == k || (placeholder && j < k));
            if names[name.as_str()] == 1
                && once
                && !exported(name)
                && unwritten(uses.get(&(false, *id)))
            {
                cand.insert((false, *id), (name.clone(), *func_id));
                decl.insert((false, *id), Item::Init(k));
            }
        }
    }
    // A function literal bound once to a local `let`/`const` inside a
    // function (decision 23), never written: a candidate as a module
    // function is (its every reference is in the module), keyed by
    // `<name>#<func id>`. A module-scope `let` is not local.
    {
        let mut all: Vec<(u32, Option<(String, u32)>)> = Vec::new();
        for k in 0..module.functions.len() {
            local_lets(module, Item::Func(k), &mut all);
        }
        for k in 0..module.globals.len() {
            local_lets(module, Item::Global(k), &mut all);
        }
        for k in 0..module.init.len() {
            local_lets(module, Item::Init(k), &mut all);
        }
        let mut count: FxHashMap<u32, u32> = FxHashMap::default();
        for (id, _) in &all {
            *count.entry(*id).or_default() += 1;
        }
        for (id, lit) in all {
            if let Some((key, func_id)) = lit {
                if !lets.contains_key(&id) && count[&id] == 1 && unwritten(uses.get(&(false, id))) {
                    cand.insert((false, id), (key, func_id));
                }
            }
        }
    }
    // Every reference is a direct callee, a TDZ check, or a direct argument
    // at a call of a closed function (decision 19: the argument contract
    // carries the function value, the callee's calls of it publish what they
    // pass; a callee that lets it escape breaks it in the census). The greatest
    // set: drop a candidate with any other reference until none changes.
    // Member accesses count when all are in the declaring unit (DECISIONS 93).
    let members = |k: &(bool, u32), u: &Use| match (unit_of, decl.get(k)) {
        (Some(unit_of), Some(&d)) if u.member_items.iter().all(|&i| unit_of(i) == unit_of(d)) => {
            u.members.saturating_sub(u.receivers)
        }
        _ => 0,
    };
    loop {
        let drop: Vec<(bool, u32)> = cand
            .keys()
            .filter(|k| {
                uses.get(k).is_some_and(|u| {
                    u.refs
                        != u.calls
                            + u.checks
                            + members(k, u)
                            + u.args_of.iter().filter(|c| cand.contains_key(c)).count() as u32
                })
            })
            .copied()
            .collect();
        if drop.is_empty() {
            break;
        }
        for k in drop {
            cand.remove(&k);
        }
    }
    // DECISIONS 195: the closed ones passed by name as a direct argument.
    let passed: FxHashSet<String> = cand
        .iter()
        .filter(|(k, _)| {
            uses.get(k)
                .is_some_and(|u| u.args_of.iter().any(|c| cand.contains_key(c)))
        })
        .map(|(_, (key, _))| key.clone())
        .collect();
    let mut out: FxHashMap<String, u32> = cand.values().cloned().collect();
    // Closed classes (design §18.13): a class with a constructor, no export
    // of the module, whose name no other module-scope binding has, with no
    // static member, which no class extends, and whose every naming
    // expression is a direct `new` of it (no value use: a spread `new` takes
    // the class as a value). Every construction is a `new` in the module or
    // through the `constructor` of an instance, both modeled where they
    // run: its constructor's parameters hold what the constructing units
    // pass (`units::ClassContract::closed`), keyed by the class name with
    // the constructor's HIR id.
    //
    // DECISIONS 157: any other class with a constructor (exported, used as a
    // value, extended, with static members) is published: every unit
    // holding it holds its stand-in (it reaches another unit only through
    // the unit boundary), whose `new` and `super(...)` publish what they
    // pass under the same key. A use of the stand-in beyond its contract is
    // that unit's census report: the class is broken, its constructor's
    // parameters ⊤. Its own unit's escapes of it other than the
    // publication construct it with ⊤ arguments as before.
    let mut published: FxHashSet<String> = FxHashSet::default();
    for c in &module.classes {
        let Some(ctor) = &c.constructor else {
            continue;
        };
        // Its key is its name: no other class or function of the module has
        // it (a CommonJS module's `export const C = _cjs.C` binds it too).
        if c.is_literal_shape()
            || module.classes.iter().filter(|d| d.name == c.name).count() != 1
            || module.functions.iter().any(|f| f.name == c.name)
            || !c.aliases.is_empty()
            || ctor.params.iter().any(|p| p.is_rest)
            || out.contains_key(&c.name)
        {
            continue;
        }
        let closed = names.get(c.name.as_str()) == Some(&1)
            && !exported(&c.name)
            && c.static_fields.is_empty()
            && c.static_methods.is_empty()
            && c.static_accessor_names.is_empty()
            && !module
                .classes
                .iter()
                .any(|d| d.extends_name.as_deref() == Some(c.name.as_str()))
            && class_uses
                .get(&c.name)
                .is_some_and(|&(refs, news)| refs == news);
        if !closed {
            published.insert(c.name.clone());
        }
        out.insert(c.name.clone(), ctor.id);
    }
    (cand, out, published, passed)
}



