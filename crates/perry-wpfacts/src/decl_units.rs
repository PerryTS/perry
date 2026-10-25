//! Declaration units inside a module (design §12.1, §12.6 steps 1-2): the
//! module scope's declarations and statements, joined along references,
//! cut into the SCCs of that graph. A unit is solved over a view of the
//! module: its own items, every other unit's declaration entering as an
//! import of that unit, exactly as another module's export does (§11).

use crate::pts::{FxHashMap, FxHashSet};
use perry_hir::tdz_check::{for_each_class_expr, for_each_expr_in, for_each_function_expr};
use perry_hir::{Export, Expr, Import, ImportSpecifier, Module, Stmt};

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
    /// Its closed functions (decision 13, [`closed_bindings`]): name, HIR
    /// id of the body.
    pub closed: FxHashMap<String, u32>,
    /// The other units' closed functions it refers to (it calls them).
    pub closed_refs: Vec<String>,
    /// The names it refers to that no unit of its module owns: its imports
    /// (and globals). The driver orders it after the units owning them
    /// (decision 21).
    pub ext_names: FxHashSet<String>,
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
/// write of the binding, a store through it, a local two items share).
#[derive(Default)]
struct Refs {
    names: Vec<String>,
    locals: Vec<u32>,
    funcs: Vec<u32>,
    globals: Vec<u32>,
    join_names: Vec<String>,
    join_locals: Vec<u32>,
    join_globals: Vec<u32>,
    /// Locals the item declares (closure parameters here; `declared` adds
    /// its functions' parameters, `let`s and catch bindings).
    declared: Vec<u32>,
    /// The closures inside the item.
    closures: Vec<u32>,
    /// The named types of its parameters (an entry guard's class, §11.4).
    param_types: Vec<String>,
}

fn param_types(params: &[perry_hir::Param], r: &mut Refs) {
    for p in params {
        if let perry_hir::types::Type::Named(n) = &p.ty {
            r.param_types.push(n.clone());
        }
    }
}

/// Every local an item declares: its functions' parameters, every `let`
/// and catch binding of their bodies (nested blocks and closures
/// included), closure parameters (`Refs::declared`).
fn declared(module: &Module, it: Item, r: &Refs) -> FxHashSet<u32> {
    use perry_hir::tdz_check::{for_each_expr_stmt_deep, for_each_stmt_deep};
    let mut d: FxHashSet<u32> = r.declared.iter().copied().collect();
    let mut on_stmt = |s: &Stmt| match s {
        Stmt::Let { id, .. } => {
            d.insert(*id);
        }
        Stmt::Try { catch: Some(c), .. } => {
            if let Some((id, _)) = &c.param {
                d.insert(*id);
            }
        }
        _ => {}
    };
    let mut fns: Vec<&perry_hir::Function> = Vec::new();
    match it {
        Item::Func(k) => fns.push(&module.functions[k]),
        Item::Class(k) => {
            let c = &module.classes[k];
            fns.extend(c.constructor.iter());
            fns.extend(c.methods.iter().chain(&c.static_methods));
            fns.extend(c.getters.iter().chain(&c.setters).map(|(_, f)| f));
            fns.extend(c.computed_members.iter().map(|m| &m.function));
            for fld in c.fields.iter().chain(&c.static_fields) {
                if let Some(e) = &fld.init {
                    for_each_expr_stmt_deep(e, &mut on_stmt);
                }
            }
        }
        Item::Global(k) => {
            if let Some(e) = &module.globals[k].init {
                for_each_expr_stmt_deep(e, &mut on_stmt);
            }
        }
        Item::Init(k) => for_each_stmt_deep(std::slice::from_ref(&module.init[k]), &mut on_stmt),
    }
    for f in &fns {
        for_each_stmt_deep(&f.body, &mut on_stmt);
    }
    for f in fns {
        d.extend(f.params.iter().map(|p| p.id));
    }
    d
}

fn direct_ref(e: &Expr, r: &mut Refs) {
    match e {
        Expr::LocalGet(id) => r.join_locals.push(*id),
        Expr::GlobalGet(id) => r.join_globals.push(*id),
        Expr::ClassRef(n) | Expr::ExternFuncRef { name: n, .. } => r.join_names.push(n.clone()),
        Expr::FuncRef(id) => r.funcs.push(*id),
        _ => {}
    }
}

fn expr_refs(e: &Expr, r: &mut Refs) {
    match e {
        Expr::LocalGet(id) => r.locals.push(*id),
        Expr::LocalSet(id, _) | Expr::Update { id, .. } => r.join_locals.push(*id),
        Expr::ArrayPush { array_id, .. }
        | Expr::ArrayPushSpread { array_id, .. }
        | Expr::ArrayUnshift { array_id, .. }
        | Expr::ArraySplice { array_id, .. }
        | Expr::ArrayCopyWithin { array_id, .. }
        | Expr::ArrayPop(array_id)
        | Expr::ArrayShift(array_id)
        | Expr::SetAdd {
            set_id: array_id, ..
        } => r.join_locals.push(*array_id),
        Expr::GlobalGet(id) => r.globals.push(*id),
        Expr::GlobalSet(id, _) => r.join_globals.push(*id),
        Expr::FuncRef(id) => r.funcs.push(*id),
        Expr::ClassRef(n) | Expr::ExternFuncRef { name: n, .. } => r.names.push(n.clone()),
        Expr::Closure {
            captures,
            mutable_captures,
            params,
            func_id,
            ..
        } => {
            r.closures.push(*func_id);
            r.declared.extend(params.iter().map(|p| p.id));
            param_types(params, r);
            // A capture is a read: `mutable_captures` are the captured
            // locals written anywhere (they need a box), not by this
            // closure; its own writes are `LocalSet`s in its body, walked
            // with the item (tsc: every function capturing `var Debug`,
            // assigned once by `Debug || (Debug = {})`, joined one unit).
            r.locals
                .extend(captures.iter().chain(mutable_captures).copied());
        }
        // A store through a declaration's name is that declaration's
        // state (`ZodString.create = …`).
        Expr::PropertySet { object, .. }
        | Expr::PropertyUpdate { object, .. }
        | Expr::IndexSet { object, .. }
        | Expr::IndexUpdate { object, .. }
        | Expr::PutValueSet { target: object, .. } => direct_ref(object, r),
        _ => class_name_refs(e, r),
    }
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

/// N5 (design §12.5): a body that runs exactly once at a statement of the
/// module scope is module scope for the cut. The callee of a call that is a
/// statement of its own or a `let`'s initializer, in a run-once position:
/// a function literal called at its own site (`((module2) => {…})(module)`),
/// a module function named nowhere else, or a local function named only at
/// that call or the CommonJS factory (the wrapper `const _cjs =
/// (function(){ function __perry_cjs_factory(){…} return
/// __perry_cjs_factory(); })()`, its module record holding the factory),
/// recursively. The statement becomes the body's statements, the parameters
/// `let`s bound to the arguments and a final `return e` the `let`'s value,
/// so the body's declarations are the module's: one instance of its locals
/// exists, as of the module's own. A body that returns anywhere but at its
/// end, reads `this`, `arguments` or `new.target`, is async or a generator,
/// or has a rest or defaulted parameter stays one declaration. `None` when
/// nothing runs once. The view serves the analysis only (sites keep their
/// offsets).
pub(crate) fn run_once_flat(module: &Module) -> Option<Module> {
    let mut names: FxHashMap<u32, usize> = FxHashMap::default();
    let mut locals: FxHashMap<u32, usize> = FxHashMap::default();
    perry_hir::tdz_check::for_each_module_expr(module, &mut |e| match e {
        Expr::FuncRef(id) => *names.entry(*id).or_default() += 1,
        // Every use of a local, in a closure body too (capture lists only
        // mirror these uses).
        Expr::LocalGet(id) | Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
            *locals.entry(*id).or_default() += 1
        }
        _ => {}
    });
    let mut flat = Flat {
        module,
        names,
        locals,
        inlined: FxHashSet::default(),
        fn_lets: FxHashMap::default(),
        out: Vec::new(),
        changed: false,
    };
    for s in &module.init {
        flat.stmt(s);
    }
    if !flat.changed {
        return None;
    }
    let mut m = module.clone();
    m.functions.retain(|f| !flat.inlined.contains(&f.id));
    m.init = flat.out.into_iter().flatten().collect();
    Some(m)
}

struct Flat<'a> {
    module: &'a Module,
    /// Uses of each module function and each local (reads, writes).
    names: FxHashMap<u32, usize>,
    locals: FxHashMap<u32, usize>,
    inlined: FxHashSet<u32>,
    /// `let f = <function literal>` statements emitted so far (index in
    /// `out`): a call of `f` named nowhere else takes the literal.
    fn_lets: FxHashMap<u32, usize>,
    out: Vec<Option<Stmt>>,
    changed: bool,
}

/// Where a call's result goes: nowhere, or a `let`.
type Dest<'s> = Option<(u32, &'s str, &'s perry_hir::types::Type, bool)>;

impl Flat<'_> {
    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Expr(Expr::Call { callee, args, .. }) if self.call(callee, args, None) => return,
            Stmt::Let {
                id,
                name,
                ty,
                mutable,
                init: Some(Expr::Call { callee, args, .. }),
            } if self.call(callee, args, Some((*id, name, ty, *mutable))) => return,
            Stmt::Let {
                id,
                init: Some(Expr::Closure { .. }),
                ..
            } => {
                self.fn_lets.insert(*id, self.out.len());
            }
            // The no-op box statements of codegen; a TDZ seed is a write of
            // each local it names (one statement per local, so each joins
            // only its own local's unit).
            Stmt::PreallocateBoxes(_) | Stmt::ReleaseBoxes(_) => {
                self.changed = true;
                return;
            }
            Stmt::PreallocateTdzBoxes(ids) if ids.len() > 1 => {
                for id in ids {
                    self.out.push(Some(Stmt::PreallocateTdzBoxes(vec![*id])));
                }
                self.changed = true;
                return;
            }
            _ => {}
        }
        self.out.push(Some(s.clone()));
    }

    /// Inline the call if its callee's body runs once here (true).
    fn call(&mut self, callee: &Expr, args: &[Expr], dest: Dest) -> bool {
        let lit = match callee {
            Expr::FuncRef(id) if self.names.get(id) == Some(&1) => {
                let m = self.module;
                let Some(f) = m.functions.iter().find(|f| f.id == *id) else {
                    return false;
                };
                if f.is_async || f.is_generator || !runs_inline(&f.params, &f.body, true) {
                    return false;
                }
                self.inlined.insert(*id);
                self.body(&f.params, args, &f.body, dest);
                return true;
            }
            Expr::LocalGet(id) => {
                let Some(&k) = self.fn_lets.get(id) else {
                    return false;
                };
                let Some(Stmt::Let {
                    init: Some(lit), ..
                }) = &self.out[k]
                else {
                    return false;
                };
                // Named only at this call (a self-call inside is a second
                // use), or the CommonJS factory: the module record holds it
                // too, the runtime's require cache, and a module body runs
                // once (the invariant perry-hir `run_once` already gives its
                // classes; `__perry_cjs_factory` no user code can name). Its
                // binding stays, `undefined` here, so the record's reference
                // keeps a declaration.
                let factory = matches!(&self.out[k], Some(Stmt::Let { name, .. }) if name == "__perry_cjs_factory");
                if self.locals.get(id) != Some(&1) && !factory {
                    return false;
                }
                let lit = lit.clone();
                if !inline_literal(&lit) {
                    return false;
                }
                self.out[k] = match self.out[k].take() {
                    Some(Stmt::Let {
                        id,
                        name,
                        ty,
                        mutable,
                        ..
                    }) if factory => Some(Stmt::Let {
                        id,
                        name,
                        ty,
                        mutable,
                        init: Some(Expr::Undefined),
                    }),
                    _ => None,
                };
                lit
            }
            Expr::Closure { .. } if inline_literal(callee) => callee.clone(),
            _ => return false,
        };
        let Expr::Closure { params, body, .. } = &lit else {
            return false;
        };
        self.body(params, args, body, dest);
        true
    }

    fn body(&mut self, params: &[perry_hir::Param], args: &[Expr], body: &[Stmt], dest: Dest) {
        self.changed = true;
        for (i, p) in params.iter().enumerate() {
            let init = args.get(i).cloned().unwrap_or(Expr::Undefined);
            self.out.push(Some(Stmt::Let {
                id: p.id,
                name: p.name.clone(),
                ty: p.ty.clone(),
                mutable: true,
                init: Some(init),
            }));
        }
        for a in args.iter().skip(params.len()) {
            self.out.push(Some(Stmt::Expr(a.clone())));
        }
        let (stmts, result) = match body.split_last() {
            Some((Stmt::Return(e), rest)) => (rest, e.clone()),
            _ => (body, None),
        };
        for s in stmts {
            self.stmt(s);
        }
        match dest {
            Some((id, name, ty, mutable)) => {
                let init = Some(result.unwrap_or(Expr::Undefined));
                self.stmt(&Stmt::Let {
                    id,
                    name: name.to_string(),
                    ty: ty.clone(),
                    mutable,
                    init,
                });
            }
            None => {
                if let Some(e) = result {
                    self.stmt(&Stmt::Expr(e));
                }
            }
        }
    }
}

/// A function literal whose body may run as statements of the module scope.
fn inline_literal(e: &Expr) -> bool {
    matches!(e, Expr::Closure { params, body, is_async: false, is_generator: false, captures_this: false, captures_new_target: false, is_arrow, .. }
        if runs_inline(params, body, !is_arrow) && !obj_literal_iife(params))
}

/// The lowering of an object literal in source order (`((__perry_obj_iife)
/// => { …; return __perry_obj_iife })({})`): an expression, whose defines
/// are the literal's birth (`Gen::call_expr`), not a body that runs once.
/// Flat, its parts would be module-scope items of their own, the literal
/// born empty in one unit and its keys stored by others (design §18.9).
pub(crate) fn obj_literal_iife(params: &[perry_hir::Param]) -> bool {
    params.len() == 1 && params[0].name == "__perry_obj_iife"
}

/// A body that can run as statements of the module scope: no `return` but
/// a final one, no `this` (own, or lexical through arrows; `own_this` false
/// for an arrow literal, whose `captures_this` the caller checks), no
/// `arguments`, plain parameters.
fn runs_inline(params: &[perry_hir::Param], body: &[Stmt], own_this: bool) -> bool {
    if params
        .iter()
        .any(|p| p.is_rest || p.default.is_some() || p.arguments_object.is_some())
    {
        return false;
    }
    let stmts = match body.split_last() {
        Some((Stmt::Return(_), rest)) => rest,
        _ => body,
    };
    let mut ok = true;
    perry_hir::tdz_check::for_each_stmt_shallow(stmts, &mut |s| {
        ok &= !matches!(s, Stmt::Return(_))
    });
    if ok && own_this {
        // `this` of the body itself, or of an arrow inside it.
        perry_hir::tdz_check::for_each_stmts_expr_shallow(body, &mut |e| {
            ok &= !matches!(
                e,
                Expr::This
                    | Expr::Closure {
                        is_arrow: true,
                        captures_this: true,
                        ..
                    }
            )
        });
    }
    ok
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

fn stmt_name(s: &Stmt) -> Option<(&str, u32)> {
    match s {
        Stmt::Let { id, name, .. } => Some((name.as_str(), *id)),
        _ => None,
    }
}

/// Decision 13: the closed functions of `module`, by name with the HIR id
/// of the body: a function declaration, or a `let` bound once to a function
/// literal, that is no export of the module, whose name no other
/// module-scope binding has, and whose every reference in the module is
/// the callee of a direct call (no spread). Every call of it is a call in
/// the module, known at compile time: its parameters hold what those calls
/// pass (`units::ArgContract`), not unknown values. None in a module with a
/// `with` statement (names resolve at run time there).
pub(crate) fn closed_bindings(module: &Module) -> FxHashMap<String, u32> {
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
        let f = &mut |e: &Expr| {
            named(e);
            match e {
                Expr::Call { callee, args, .. } => {
                    if let Some(id) = perry_hir::tdz_check::checked_binding(e) {
                        uses.entry((false, id)).or_default().checks += 1;
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
        for func in &module.functions {
            for_each_function_expr(func, f);
        }
        for c in &module.classes {
            for_each_class_expr(c, f);
        }
        for g in &module.globals {
            if let Some(e) = &g.init {
                for_each_expr_in(e, f);
            }
        }
        perry_hir::tdz_check::for_each_stmts_expr(&module.init, f);
    }
    if with {
        return FxHashMap::default();
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
        module
            .exports
            .iter()
            .any(|e| matches!(e, Export::Named { local, .. } if local == name))
            || module.exported_objects.iter().any(|n| n == name)
    };
    // The candidates: unexported, uniquely named, never written.
    let mut cand: FxHashMap<(bool, u32), (String, u32)> = FxHashMap::default();
    let unwritten = |u: Option<&Use>| u.is_none_or(|u| !u.written);
    for func in &module.functions {
        let named = |l: &[(String, u32)]| l.iter().any(|(_, id)| *id == func.id);
        if names[func.name.as_str()] == 1
            && !exported(&func.name)
            && !named(&module.exported_functions)
            && !named(&module.script_global_functions)
            && unwritten(uses.get(&(true, func.id)))
        {
            cand.insert((true, func.id), (func.name.clone(), func.id));
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
    loop {
        let drop: Vec<(bool, u32)> = cand
            .keys()
            .filter(|k| {
                uses.get(k).is_some_and(|u| {
                    u.refs
                        != u.calls
                            + u.checks
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
    let mut out: FxHashMap<String, u32> = cand.into_values().collect();
    // Closed classes (design §18.13): a class with a constructor, no export
    // of the module, whose name no other module-scope binding has, with no
    // static member, which no class extends, and whose every naming
    // expression is a direct `new` of it (no value use: a spread `new` takes
    // the class as a value). Every construction is a `new` in the module or
    // through the `constructor` of an instance, both modeled where they
    // run: its constructor's parameters hold what the constructing units
    // pass (`units::ClassContract::closed`), keyed by the class name with
    // the constructor's HIR id.
    for c in &module.classes {
        let Some(ctor) = &c.constructor else {
            continue;
        };
        if c.is_literal_shape()
            || names.get(c.name.as_str()) != Some(&1)
            || exported(&c.name)
            || !c.aliases.is_empty()
            || !c.static_fields.is_empty()
            || !c.static_methods.is_empty()
            || !c.static_accessor_names.is_empty()
            || ctor.params.iter().any(|p| p.is_rest)
            || module
                .classes
                .iter()
                .any(|d| d.extends_name.as_deref() == Some(c.name.as_str()))
            || out.contains_key(&c.name)
        {
            continue;
        }
        if class_uses
            .get(&c.name)
            .is_some_and(|&(refs, news)| refs == news)
        {
            out.insert(c.name.clone(), ctor.id);
        }
    }
    out
}

/// The declaration units of `module`, in dependency order (a unit before
/// the units referring to it). `one` gives the whole module as one unit
/// (P2).
pub(crate) fn decl_units(module: &Module) -> Vec<Vec<Item>> {
    let mut items: Vec<Item> = Vec::new();
    items.extend((0..module.functions.len()).map(Item::Func));
    // A literal-shape class (the shape-only class of a closed object
    // literal: no code, no state, `Class::is_literal_shape`) is a layout,
    // not a declaration: every unit that births it gets its own copy
    // (`views`), as two modules with one shape do; their templates share
    // the class id, so a site's guard is stable only when every copy is.
    items.extend(
        (0..module.classes.len())
            .filter(|&k| !module.classes[k].is_literal_shape())
            .map(Item::Class),
    );
    items.extend((0..module.globals.len()).map(Item::Global));
    items.extend((0..module.init.len()).map(Item::Init));
    let n = items.len();
    let mut by_name: FxHashMap<&str, Vec<usize>> = FxHashMap::default();
    let mut by_local: FxHashMap<u32, Vec<usize>> = FxHashMap::default();
    let mut by_func: FxHashMap<u32, usize> = FxHashMap::default();
    let mut by_global: FxHashMap<u32, usize> = FxHashMap::default();
    for (i, it) in items.iter().enumerate() {
        match *it {
            Item::Func(k) => {
                let f = &module.functions[k];
                by_name.entry(f.name.as_str()).or_default().push(i);
                by_func.insert(f.id, i);
            }
            Item::Class(k) => {
                let c = &module.classes[k];
                by_name.entry(c.name.as_str()).or_default().push(i);
                for a in &c.aliases {
                    by_name.entry(a.as_str()).or_default().push(i);
                }
            }
            Item::Global(k) => {
                let g = &module.globals[k];
                by_name.entry(g.name.as_str()).or_default().push(i);
                by_global.insert(g.id, i);
            }
            Item::Init(k) => {
                // A `let` is its id: two module-scope statements of one id
                // (a `var` hoisted, then assigned) are one binding; two of
                // one name and distinct ids are two (locals of two scopes a
                // once-run body brought to module scope, N5).
                if let Some((_, id)) = stmt_name(&module.init[k]) {
                    by_local.entry(id).or_default().push(i);
                }
            }
        }
    }
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    // Two declarations of one name are one binding; so are two statements
    // declaring one local.
    for v in by_name.values().chain(by_local.values()) {
        for w in v.windows(2) {
            succ[w[0]].push(w[1]);
            succ[w[1]].push(w[0]);
        }
    }
    // A local no module-scope declaration owns: an item declaring it owns
    // it; an item reading it without declaring it shares the declaring
    // item's binding (a function lifted out of the function declaring the
    // local, a nested class), one unit. The same local declared in two
    // items is two bindings (an inlined body is a copy, not shared state).
    // A local no item declares joins every item referring to it.
    let mut inner_refs: FxHashMap<u32, Vec<usize>> = FxHashMap::default();
    let mut closure_owner = FxHashMap::default();
    let mut decl_of: FxHashMap<u32, Vec<usize>> = FxHashMap::default();
    let mut free: Vec<(usize, u32)> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let mut r = Refs::default();
        {
            let f = &mut |e: &Expr| expr_refs(e, &mut r);
            match *it {
                Item::Func(k) => for_each_function_expr(&module.functions[k], f),
                Item::Class(k) => {
                    let c = &module.classes[k];
                    for_each_class_expr(c, f);
                }
                Item::Global(k) => {
                    if let Some(e) = &module.globals[k].init {
                        for_each_expr_in(e, f);
                    }
                }
                Item::Init(k) => perry_hir::tdz_check::for_each_stmts_expr(
                    std::slice::from_ref(&module.init[k]),
                    f,
                ),
            }
        }
        if let Item::Class(k) = *it {
            let c = &module.classes[k];
            if let Some(p) = &c.extends_name {
                r.names.push(p.clone());
            }
        }
        // A TDZ seed writes its locals.
        if let Item::Init(k) = *it {
            if let Stmt::PreallocateTdzBoxes(ids) = &module.init[k] {
                r.join_locals.extend(ids.iter().copied());
            }
        }
        let mut edge = |j: usize, both: bool| {
            if j != i {
                succ[i].push(j);
                if both {
                    succ[j].push(i);
                }
            }
        };
        for (list, both) in [(&r.names, false), (&r.join_names, true)] {
            for nm in list {
                for &j in by_name.get(nm.as_str()).into_iter().flatten() {
                    edge(j, both);
                }
            }
        }
        for (list, both) in [(&r.locals, false), (&r.join_locals, true)] {
            for id in list {
                match by_local.get(id) {
                    Some(js) => js.iter().for_each(|&j| edge(j, both)),
                    None => {
                        let v = inner_refs.entry(*id).or_default();
                        if v.last() != Some(&i) {
                            v.push(i);
                        }
                    }
                }
            }
        }
        // Init items split ONE emitted lexical frame. A nested var
        // initializer (for example under try) writes its hoisted binding;
        // unlike a copied function frame, it is not a second declaration.
        if let Item::Init(k) = *it {
            perry_hir::tdz_check::for_each_stmt_shallow(
                std::slice::from_ref(&module.init[k]),
                &mut |s| {
                    if let Stmt::Let { id, .. } = s {
                        for &j in by_local.get(id).into_iter().flatten() {
                            edge(j, true);
                        }
                    }
                },
            );
        }
        // Repeated creation expressions with this FuncId share ONE emitted
        // body. Its receiver/capture facts must cover every creation/caller.
        for id in &r.closures {
            if let Some(&j) = closure_owner.get(id) {
                edge(j, true);
            } else {
                closure_owner.insert(*id, i);
            }
        }
        let own = declared(module, *it, &r);
        for &id in &own {
            decl_of.entry(id).or_default().push(i);
        }
        for id in r.locals.iter().chain(&r.join_locals) {
            if !by_local.contains_key(id) && !own.contains(id) {
                free.push((i, *id));
            }
        }
        for id in &r.funcs {
            if let Some(&j) = by_func.get(id) {
                edge(j, false);
            }
        }
        for (list, both) in [(&r.globals, false), (&r.join_globals, true)] {
            for id in list {
                if let Some(&j) = by_global.get(id) {
                    edge(j, both);
                }
            }
        }
    }
    for (i, id) in free {
        let with = decl_of.get(&id).or_else(|| inner_refs.get(&id));
        for &j in with.into_iter().flatten() {
            if j != i {
                succ[i].push(j);
                succ[j].push(i);
            }
        }
    }
    let out: Vec<Vec<Item>> = sccs(&succ)
        .into_iter()
        .map(|c| c.into_iter().map(|i| items[i]).collect())
        .collect();
    out
}

/// Tarjan's SCCs, iterative; emitted callee-first (an SCC after every SCC
/// it refers to), which is the dependency order.
fn sccs(succ: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = succ.len();
    let (mut index, mut low) = (vec![u32::MAX; n], vec![0u32; n]);
    let mut on = vec![false; n];
    let (mut stack, mut out, mut next) = (Vec::new(), Vec::new(), 0u32);
    for root in 0..n {
        if index[root] != u32::MAX {
            continue;
        }
        let mut call: Vec<(usize, usize)> = vec![(root, 0)];
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on[root] = true;
        while let Some(&mut (v, ref mut e)) = call.last_mut() {
            if *e < succ[v].len() {
                let w = succ[v][*e];
                *e += 1;
                if index[w] == u32::MAX {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on[w] = true;
                    call.push((w, 0));
                } else if on[w] {
                    low[v] = low[v].min(index[w]);
                }
            } else {
                call.pop();
                if let Some(&(u, _)) = call.last() {
                    low[u] = low[u].min(low[v]);
                }
                if low[v] == index[v] {
                    let mut c = Vec::new();
                    loop {
                        let w = stack.pop().expect("tarjan stack");
                        on[w] = false;
                        c.push(w);
                        if w == v {
                            break;
                        }
                    }
                    c.sort_unstable();
                    out.push(c);
                }
            }
        }
    }
    out
}

/// The views of `module`'s declaration units (in dependency order): per
/// unit a module with its own items, the module's imports, synthetic
/// imports of the other units' declarations it refers to (from
/// [`unit_path`]), its exports, and synthetic exports of its declarations
/// other units refer to. Also the units each unit refers to.
pub(crate) fn views<'a>(path: &str, module: &'a Module) -> Vec<(UnitView<'a>, Vec<usize>)> {
    let closure_bodies: FxHashMap<_, _> = perry_hir::emitted_closures(module).into_iter().collect();
    let units = decl_units(module);
    let closed = closed_bindings(module);
    let mut owner_name: FxHashMap<&str, usize> = FxHashMap::default();
    let mut owner_local: FxHashMap<u32, (usize, &str)> = FxHashMap::default();
    let mut owner_func: FxHashMap<u32, (usize, &str)> = FxHashMap::default();
    let mut owner_global: FxHashMap<u32, (usize, &str)> = FxHashMap::default();
    let mut owner_class: FxHashMap<&str, usize> = FxHashMap::default();
    for (u, items) in units.iter().enumerate() {
        for it in items {
            match *it {
                Item::Func(k) => {
                    let f = &module.functions[k];
                    owner_name.insert(&f.name, u);
                    owner_func.insert(f.id, (u, &f.name));
                }
                Item::Class(k) => {
                    let c = &module.classes[k];
                    owner_name.insert(&c.name, u);
                    owner_class.insert(&c.name, u);
                    for a in &c.aliases {
                        owner_name.insert(a, u);
                    }
                }
                Item::Global(k) => {
                    let g = &module.globals[k];
                    owner_name.insert(&g.name, u);
                    owner_global.insert(g.id, (u, &g.name));
                }
                Item::Init(k) => {
                    if let Some((name, id)) = stmt_name(&module.init[k]) {
                        owner_name.insert(name, u);
                        owner_local.insert(id, (u, name));
                    }
                }
            }
        }
    }
    // Per unit: the foreign names it refers to.
    let mut wanted: Vec<FxHashSet<String>> = vec![FxHashSet::default(); units.len()];
    let mut imports_of: Vec<FxHashSet<(usize, String)>> = vec![FxHashSet::default(); units.len()];
    let shapes: FxHashMap<&str, usize> = module
        .classes
        .iter()
        .enumerate()
        .filter(|(_, c)| c.is_literal_shape())
        .map(|(k, c)| (c.name.as_str(), k))
        .collect();
    // A view is the unit's item indices and its own imports/exports: the
    // module itself is read through the one `Module` every view borrows,
    // so the views together cost their index lists, not units × module.
    let mut out: Vec<(UnitView, Vec<usize>)> = Vec::with_capacity(units.len());
    for (u, items) in units.iter().enumerate() {
        let mut view = UnitView::default();
        for it in items {
            let own = match *it {
                Item::Func(k) => Some((module.functions[k].name.as_str(), module.functions[k].id)),
                Item::Init(k) => match &module.init[k] {
                    Stmt::Let {
                        name,
                        init: Some(Expr::Closure { func_id, .. }),
                        ..
                    } => Some((name.as_str(), *func_id)),
                    _ => None,
                },
                // A closed class: by its constructor (design §18.13).
                Item::Class(k) => {
                    let c = &module.classes[k];
                    c.constructor.as_ref().map(|f| (c.name.as_str(), f.id))
                }
                _ => None,
            };
            if let Some((nm, id)) = own.filter(|(nm, id)| closed.get(*nm) == Some(id)) {
                view.closed.insert(nm.to_string(), id);
            }
            // Its closed locals (decision 23): the unit that runs them.
            let mut ls: Vec<(u32, Option<(String, u32)>)> = Vec::new();
            local_lets(module, *it, &mut ls);
            for (key, id) in ls.into_iter().filter_map(|(_, l)| l) {
                if closed.get(&key) == Some(&id) {
                    view.closed.insert(key, id);
                }
            }
        }
        let mut r = Refs::default();
        {
            let f = &mut |e: &Expr| expr_refs(e, &mut r);
            for it in items {
                match *it {
                    Item::Func(k) => {
                        for_each_function_expr(&module.functions[k], f);
                        view.funcs.push(k);
                    }
                    Item::Class(k) => {
                        for_each_class_expr(&module.classes[k], f);
                        view.classes.push(k);
                    }
                    Item::Global(k) => {
                        if let Some(e) = &module.globals[k].init {
                            for_each_expr_in(e, f);
                        }
                        view.globals.push(k);
                    }
                    Item::Init(k) => {
                        perry_hir::tdz_check::for_each_stmts_expr(
                            std::slice::from_ref(&module.init[k]),
                            f,
                        );
                        view.init.push(k);
                    }
                }
            }
        }
        view.closure_bodies.extend(
            r.closures
                .iter()
                .filter_map(|id| closure_bodies.get(id).map(|&body| (*id, body))),
        );
        view.binding_funcs.extend(r.closures.iter().copied());
        for it in items {
            view.binding_locals.extend(declared(module, *it, &r));
            match *it {
                Item::Func(k) => {
                    view.binding_funcs.insert(module.functions[k].id);
                }
                Item::Class(k) => {
                    let c = &module.classes[k];
                    view.binding_funcs.extend(
                        c.constructor
                            .iter()
                            .chain(&c.methods)
                            .chain(&c.static_methods)
                            .chain(c.getters.iter().chain(&c.setters).map(|(_, f)| f))
                            .chain(c.computed_members.iter().map(|m| &m.function))
                            .map(|f| f.id),
                    );
                }
                _ => {}
            }
        }
        for it in items {
            match *it {
                Item::Func(k) => param_types(&module.functions[k].params, &mut r),
                Item::Class(k) => {
                    let c = &module.classes[k];
                    if let Some(p) = &c.extends_name {
                        r.names.push(p.clone());
                    }
                    let fs = c
                        .constructor
                        .iter()
                        .chain(&c.methods)
                        .chain(&c.static_methods);
                    for f in fs.chain(c.getters.iter().chain(&c.setters).map(|(_, f)| f)) {
                        param_types(&f.params, &mut r);
                    }
                }
                Item::Init(k) => {
                    if let Stmt::PreallocateTdzBoxes(ids) = &module.init[k] {
                        r.join_locals.extend(ids.iter().copied());
                    }
                }
                _ => {}
            }
        }
        // The literal shapes this unit births: it sees them (the one class,
        // by index; the shared class id rule covers every unit naming it).
        let mut seen: FxHashSet<usize> = FxHashSet::default();
        for nm in r.names.iter().chain(&r.join_names) {
            if let Some(&k) = shapes.get(nm.as_str()) {
                if seen.insert(k) {
                    view.classes.push(k);
                }
            }
        }
        view.ext_names = r
            .names
            .iter()
            .chain(&r.join_names)
            .chain(&r.param_types)
            .filter(|nm| !owner_name.contains_key(nm.as_str()))
            .cloned()
            .collect();
        let mut deps: FxHashSet<usize> = FxHashSet::default();
        let imp = &mut imports_of[u];
        let mut want = |name: &str, w: usize, deps: &mut FxHashSet<usize>| {
            deps.insert(w);
            wanted[w].insert(name.to_string());
            imp.insert((w, name.to_string()));
            name.to_string()
        };
        for nm in r.names.iter().chain(&r.join_names) {
            if let Some(&w) = owner_name.get(nm.as_str()).filter(|&&w| w != u) {
                want(nm, w, &mut deps);
            }
        }
        // A parameter typed by another unit's class: the entry guard checks
        // against that class's contract (§11.4), so the unit imports the
        // class and depends on its unit. A type reference reads no state:
        // no join, no SCC edge.
        for nm in &r.param_types {
            if let Some(&w) = owner_class.get(nm.as_str()).filter(|&&w| w != u) {
                want(nm, w, &mut deps);
            }
        }
        for id in r.locals.iter().chain(&r.join_locals) {
            if let Some(&(w, nm)) = owner_local.get(id).filter(|(w, _)| *w != u) {
                view.foreign_locals.insert(*id, want(nm, w, &mut deps));
            }
        }
        for id in &r.funcs {
            if let Some(&(w, nm)) = owner_func.get(id).filter(|(w, _)| *w != u) {
                view.foreign_funcs.insert(*id, want(nm, w, &mut deps));
            }
        }
        for id in r.globals.iter().chain(&r.join_globals) {
            if let Some(&(w, nm)) = owner_global.get(id).filter(|(w, _)| *w != u) {
                view.foreign_globals.insert(*id, want(nm, w, &mut deps));
            }
        }
        // The module-wide item tables the analysis reads: its own entries.
        // (Tables keyed by an item id, `closure_display_names`, are read
        // from the module: a unit asks only for its own ids.)
        let own_fn = |id: &u32| owner_func.get(id).is_some_and(|(w, _)| *w == u);
        let own_name = |n: &str| owner_name.get(n) == Some(&u);
        view.exported_objects = module
            .exported_objects
            .iter()
            .filter(|n| own_name(n))
            .cloned()
            .collect();
        view.exported_functions = module
            .exported_functions
            .iter()
            .filter(|(_, id)| own_fn(id))
            .cloned()
            .collect();
        let mut deps: Vec<usize> = deps.into_iter().collect();
        deps.sort_unstable();
        out.push((view, deps));
    }
    // Imports of the other units' names; exports of the own ones.
    let real_owned = |u: usize, local: &str| owner_name.get(local) == Some(&u);
    for (u, (view, _)) in out.iter_mut().enumerate() {
        let mut per: FxHashMap<usize, Vec<String>> = FxHashMap::default();
        for (w, nm) in std::mem::take(&mut imports_of[u]) {
            if closed.contains_key(&nm) {
                view.closed_refs.push(nm.clone());
            }
            per.entry(w).or_default().push(nm);
        }
        view.closed_refs.sort();
        let mut per: Vec<(usize, Vec<String>)> = per.into_iter().collect();
        per.sort();
        for (_, names) in per.iter_mut() {
            names.sort();
        }
        for (w, names) in per {
            let p = unit_path(path, w);
            view.imports.push(Import {
                source: p.clone(),
                resolved_path: Some(p),
                specifiers: names
                    .into_iter()
                    .map(|n| ImportSpecifier::Named {
                        imported: n.clone(),
                        local: n,
                    })
                    .collect(),
                is_native: false,
                module_kind: Default::default(),
                type_only: false,
                runtime_erased: false,
                is_dynamic: false,
                is_dynamic_target: false,
                is_deferred_require: false,
                is_adopted_require: false,
            });
        }
        view.exports = module
            .exports
            .iter()
            .filter(|e| match e {
                Export::Named { local, .. } => real_owned(u, local),
                // Re-exports go with the first unit.
                _ => u == 0,
            })
            .cloned()
            .collect();
        let mut own: Vec<&String> = wanted[u].iter().collect();
        own.sort();
        for nm in own {
            if !view
                .exports
                .iter()
                .any(|e| matches!(e, Export::Named { exported, .. } if exported == nm))
            {
                view.unit_exports.insert(nm.clone());
            }
            view.exports.push(Export::Named {
                local: nm.clone(),
                exported: nm.clone(),
            });
        }
    }
    out
}
