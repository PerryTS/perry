//! FVA 4.3b: a for-in body is a function of its key.
//!
//! HIR lowers `for (k in o) body` to `__obj = o; __keys = ForInKeys(__obj);
//! for (i …) if (guard) { const k = __keys[i]; body }`. Generation makes the
//! body a function of the key (`for_in_body`) and the loop a call of it per
//! value of `__obj` (`Cons::ForInBody`). The solve runs it once per key of a
//! literal whose keys are exactly its birth keys, that run's key parameter
//! holding the one constant key (`Model::key_nodes`), so `o[k]`, `o[k] = v`
//! and `defineProperty(o, k, d)` in it read, store and define that key; any
//! other value runs it once with every key, as the loop did before.

use super::*;
use perry_hir::tdz_check::{
    for_each_class_expr, for_each_expr_in, for_each_function_expr, for_each_stmt_deep,
    for_each_stmt_shallow, for_each_stmts_expr, for_each_stmts_expr_shallow,
};
use perry_hir::{Class, Expr, Function, Stmt};

/// A for-in loop as HIR lowers it.
pub(crate) struct ForIn<'s> {
    /// The iterated object's local (`__forin_obj`).
    pub obj: u32,
    /// The loop variable.
    pub key: u32,
    pub guard: &'s Expr,
    /// `const k = __keys[i]`.
    pub bind: &'s Stmt,
    pub body: &'s [Stmt],
}

/// `stmts` (the `For` body) as a for-in over the keys local `keys` of
/// object local `obj` (`Gen::for_in_keys_of`): `if (guard) { const k =
/// __keys[i]; body }`.
fn for_in_shape<'s>(body: &'s [Stmt], keys_of: impl Fn(u32) -> Option<u32>) -> Option<ForIn<'s>> {
    let [Stmt::If {
        condition,
        then_branch,
        else_branch: None,
    }] = body
    else {
        return None;
    };
    let (bind, rest) = then_branch.split_first()?;
    let Stmt::Let {
        id,
        init: Some(Expr::IndexGet { object, index, .. }),
        ..
    } = bind
    else {
        return None;
    };
    let (Expr::LocalGet(keys), Expr::LocalGet(_)) = (object.as_ref(), index.as_ref()) else {
        return None;
    };
    Some(ForIn {
        obj: keys_of(*keys)?,
        key: *id,
        guard: condition,
        bind,
        body: rest,
    })
}

/// The body runs in the enclosing function's frame: it neither returns
/// from it nor suspends it, and reads no frame state a function of its own
/// would not see (`super`, `new.target`).
fn runs_in_place(body: &[Stmt]) -> bool {
    let mut ok = true;
    for_each_stmt_shallow(body, &mut |s| {
        if matches!(s, Stmt::Return(_)) {
            ok = false;
        }
    });
    for_each_stmts_expr_shallow(body, &mut |e| {
        let v = super::walk::variant_name(e);
        if v == "Await" || v == "Yield" || v.starts_with("Super") || v.contains("NewTarget") {
            ok = false;
        }
    });
    ok
}

/// `defineProperty(o, LocalGet(key), d)` (or `Reflect.defineProperty`).
fn defines_under(e: &Expr, key: u32) -> bool {
    match e {
        Expr::ObjectDefineProperty(_, k, _) | Expr::ReflectDefineProperty { key: k, .. } => {
            matches!(k.as_ref(), Expr::LocalGet(id) if *id == key)
        }
        _ => false,
    }
}

/// Every statement list of `body`, nested lists included (not those of
/// closures: a closure is a function of its own).
fn stmt_lists<'s>(body: &'s [Stmt], out: &mut Vec<&'s [Stmt]>) {
    out.push(body);
    for s in body {
        match s {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                stmt_lists(then_branch, out);
                if let Some(e) = else_branch {
                    stmt_lists(e, out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                stmt_lists(body, out)
            }
            Stmt::Labeled { body, .. } => stmt_lists(std::slice::from_ref(body.as_ref()), out),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                stmt_lists(body, out);
                if let Some(c) = catch {
                    stmt_lists(&c.body, out);
                }
                if let Some(f) = finally {
                    stmt_lists(f, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    stmt_lists(&c.body, out);
                }
            }
            _ => {}
        }
    }
}

/// The for-in loops of `body` whose body defines a property under its key:
/// (what `__obj` is initialized from, the loop).
fn defining_loops<'s>(body: &'s [Stmt]) -> Vec<(&'s Expr, ForIn<'s>)> {
    let mut lists = Vec::new();
    stmt_lists(body, &mut lists);
    let mut out = Vec::new();
    for l in lists {
        for w in l.windows(3) {
            let [Stmt::Let {
                id: obj,
                init: Some(src),
                ..
            }, Stmt::Let {
                id: keys,
                init: Some(Expr::ForInKeys(of)),
                ..
            }, Stmt::For { body: fb, .. }] = w
            else {
                continue;
            };
            if !matches!(of.as_ref(), Expr::LocalGet(o) if o == obj) {
                continue;
            }
            let Some(f) = for_in_shape(fb, |k| (k == *keys).then_some(*obj)) else {
                continue;
            };
            let mut defines = false;
            for_each_stmts_expr_shallow(f.body, &mut |e| defines |= defines_under(e, f.key));
            if defines {
                out.push((src, f));
            }
        }
    }
    out
}

impl<'a> Gen<'a> {
    /// The loop `for (…) body` as a for-in this module recorded the keys of
    /// (`for_in_keys_of`), when its body can be a function of the key:
    /// nothing in the module writes or captures the key (a run's key is a
    /// constant) and the body runs in the enclosing frame (`runs_in_place`).
    /// A constructor's loop stays a loop (its stores are construction-phase
    /// writers of `this`, decided per function).
    pub(crate) fn for_in_of<'s>(&mut self, fx: &Fx, body: &'s [Stmt]) -> Option<ForIn<'s>> {
        let mi = fx.module;
        // A module-level variable another unit may write (a unit solve sees
        // only its own items' writes).
        let fi = &self.m.funcs[fx.f as usize];
        let top_level = fi.parent == NO_FN && fi.func_id == u32::MAX;
        if fx.ctor_of.is_some() || (top_level && self.unit_view(mi as usize).is_some()) {
            return None;
        }
        let f = for_in_shape(body, |k| self.for_in_keys_of.get(&(mi, k)).copied())?;
        if self.unstable_locals(mi).contains(&f.key) || !runs_in_place(f.body) {
            return None;
        }
        Some(f)
    }

    /// Locals a run's key could not be: written anywhere this solve walks
    /// (`=`, `++`) or captured by a closure (which reads the binding later,
    /// not the run's key). A loop's variable is its function's, so its writes
    /// are in the items that function is part of.
    fn unstable_locals(&mut self, mi: u32) -> &crate::pts::FxHashSet<u32> {
        if !self.unstable.contains_key(&mi) {
            let mut s = crate::pts::FxHashSet::default();
            self.walked(mi as usize).each_expr(&mut |e| match e {
                Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
                    s.insert(*id);
                }
                Expr::Closure {
                    captures,
                    mutable_captures,
                    ..
                } => s.extend(captures.iter().chain(mutable_captures).copied()),
                _ => {}
            });
            self.unstable.insert(mi, s);
        }
        &self.unstable[&mi]
    }

    /// The loop of `f`: its key binding in the enclosing function (the local
    /// holds every key), the body as a function of the key run per value of
    /// the object.
    pub(crate) fn for_in_body(&mut self, fx: &Fx, f: &ForIn<'_>) {
        let mi = fx.module;
        let _ = self.expr(fx, f.guard);
        self.stmt(fx, f.bind);
        let g = self.m.funcs.len() as FnIdx;
        let tok = self.m.tok(TokKind::Func(g, NO_CTX));
        self.m.closures.insert(g);
        let outer = std::mem::replace(&mut self.m.cur_fn, g);
        let this = self.m.node_named("this", || format!("this(for-in#{g})"));
        let ret = self.m.node_named("ret", || format!("ret(for-in#{g})"));
        let args_all = self.m.node_named("args", || format!("args(for-in#{g})"));
        self.m.own(ret);
        self.m.own(this);
        let key = self
            .m
            .node_named("forin-key", || format!("key(for-in#{g})"));
        self.m.own(key);
        self.m.for_in_key_vars.insert(key);
        self.m.cur_fn = outer;
        self.m.funcs.push(FuncInfo {
            module: mi,
            func_id: u32::MAX,
            name: format!("{}:for-in#{g}", self.mods[mi as usize].path_short()),
            params: vec![key],
            rest: None,
            this,
            ret,
            args_all,
            has_arguments: false,
            token: tok,
            ctor_of: None,
            lexical_this: true,
            no_summary: false,
            gen_step: false,
            captures: Vec::new(),
            forwards_super: false,
            parent: fx.f,
            forwarded: false,
        });
        // The enclosing `this`, read as an arrow reads it.
        if f.body.iter().any(crate::gen_call::stmt_reads_this) {
            self.m.edge(fx.this, this);
            self.m.captures.insert((fx.this, g));
        }
        let bfx = Fx {
            guard: None,
            f: g,
            this,
            ret,
            ctor_of: None,
            fn_ctor: false,
            ..fx.clone()
        };
        self.walked.insert(g);
        self.for_in_key.push((mi, f.key, key));
        self.stmts(&bfx, f.body);
        self.for_in_key.pop();
        let on = self.local(mi, f.obj);
        let gn = self.tok_node(tok);
        let k = self.tmp("forin-key");
        self.m.add_prim(k, prim::STR);
        let (call, _) = self.new_call(ThisArg::Undef, vec![k], vec![]);
        self.m.cons(gn, Cons::ForInBody { obj: on, call });
    }

    /// The key parameter of the for-in body being walked when `id` is its
    /// loop variable (a nested body's read of an outer key captures it).
    pub(crate) fn for_in_key_param(&mut self, mi: u32, id: u32) -> Option<NodeId> {
        let &(_, _, n) = self
            .for_in_key
            .iter()
            .rev()
            .find(|&&(m, k, _)| m == mi && k == id)?;
        if self.m.node_fn[n as usize] != self.m.cur_fn && self.m.cur_fn != NO_FN {
            let f = self.m.cur_fn;
            self.m.captures.insert((n, f));
        }
        Some(n)
    }

    /// The key parameter of the innermost for-in body being walked when `e`
    /// reads its loop variable: the node a keyed rule decides on (a read of
    /// a local is a node of its own, fed by the local).
    pub(crate) fn for_in_key_of(&self, mi: u32, e: &Expr) -> Option<NodeId> {
        match (e, self.for_in_key.last()) {
            (Expr::LocalGet(id), Some(&(m, k, n))) if m == mi && *id == k => Some(n),
            _ => None,
        }
    }

    /// The keys a for-in run may define a property under, named before any
    /// body is walked: a lookup walks accessors only for keys generation
    /// knows (`Model::accessor_atoms`), so a run's definition of a key no
    /// lookup walks would be missed. They are the keys of the literals such
    /// a loop iterates: a literal it spills directly, or one passed for the
    /// parameter the loop iterates (`__export(target, { k: … })`). A run for
    /// any other key is not made (`Model::for_in_keys`).
    pub(crate) fn note_for_in_define_keys(&mut self) {
        for mi in 0..self.modules.len() {
            let w = self.walked(mi);
            let mut lits: Vec<String> = Vec::new();
            let mut iterated: crate::pts::FxHashSet<(u32, usize)> = Default::default();
            {
                let mut scan = |fid: Option<u32>, params: Vec<u32>, body: &[Stmt]| {
                    for (src, _) in defining_loops(body) {
                        match src {
                            Expr::New { class_name, .. } => lits.push(class_name.clone()),
                            Expr::LocalGet(p) => {
                                if let (Some(fid), Some(i)) =
                                    (fid, params.iter().position(|q| q == p))
                                {
                                    iterated.insert((fid, i));
                                }
                            }
                            _ => {}
                        }
                    }
                };
                match self.unit_view(mi) {
                    None => scan(None, Vec::new(), &self.modules[mi].1.init),
                    Some(_) => {
                        for s in &w.init {
                            scan(None, Vec::new(), std::slice::from_ref(*s));
                        }
                    }
                }
                for f in &w.fns {
                    scan(Some(f.id), f.params.iter().map(|p| p.id).collect(), &f.body);
                }
                w.each_expr(&mut |e| {
                    if let Expr::Closure {
                        func_id,
                        params,
                        body,
                        ..
                    } = e
                    {
                        scan(Some(*func_id), params.iter().map(|p| p.id).collect(), body);
                    }
                });
            }
            if !iterated.is_empty() {
                // Locals bound to a closure; the literals passed for an
                // iterated parameter of a function or of such a closure.
                let mut bound: FxHashMap<u32, u32> = FxHashMap::default();
                let mut note = |s: &Stmt| {
                    if let Stmt::Let {
                        id,
                        init: Some(Expr::Closure { func_id, .. }),
                        ..
                    } = s
                    {
                        bound.insert(*id, *func_id);
                    }
                };
                for s in &w.init {
                    for_each_stmt_deep(std::slice::from_ref(*s), &mut note);
                }
                for f in &w.fns {
                    for_each_stmt_deep(&f.body, &mut note);
                }
                w.each_expr(&mut |e| {
                    let Expr::Call { callee, args, .. } = e else {
                        return;
                    };
                    let fid = match callee.as_ref() {
                        Expr::FuncRef(f) => *f,
                        Expr::LocalGet(l) => match bound.get(l) {
                            Some(&f) => f,
                            None => return,
                        },
                        _ => return,
                    };
                    for (i, a) in args.iter().enumerate() {
                        if let (true, Expr::New { class_name, .. }) =
                            (iterated.contains(&(fid, i)), a)
                        {
                            lits.push(class_name.clone());
                        }
                    }
                });
            }
            for name in lits {
                for n in self.literal_keys(mi, &name) {
                    let a = self.m.atom(&n);
                    self.m.add_accessor_atom(a);
                    self.m.for_in_define_keys.insert(a);
                }
            }
        }
    }
}

/// The HIR a solve walks of one module: a unit's items (a unit view borrows
/// the whole module), else the module's.
pub(super) struct Walked<'a> {
    /// Declarations and class members (closures are found as expressions).
    pub(super) fns: Vec<&'a Function>,
    decls: Vec<&'a Function>,
    classes: Vec<&'a Class>,
    pub(super) init: Vec<&'a Stmt>,
    globals: Vec<&'a Expr>,
}

impl<'a> Walked<'a> {
    /// Every expression, closure bodies included.
    pub(super) fn each_expr(&self, f: &mut dyn FnMut(&Expr)) {
        for func in &self.decls {
            for_each_function_expr(func, f);
        }
        for c in &self.classes {
            for_each_class_expr(c, f);
        }
        for s in &self.init {
            for_each_stmts_expr(std::slice::from_ref(*s), f);
        }
        for e in &self.globals {
            for_each_expr_in(e, f);
        }
    }
}

impl<'a> Gen<'a> {
    pub(super) fn walked(&self, mi: usize) -> Walked<'a> {
        let classes = self.classes_of(mi);
        let decls = self.functions_of(mi);
        let mut fns = decls.clone();
        for c in &classes {
            fns.extend(c.constructor.iter());
            fns.extend(c.methods.iter().chain(&c.static_methods));
            fns.extend(c.getters.iter().chain(&c.setters).map(|(_, f)| f));
            fns.extend(c.computed_members.iter().map(|m| &m.function));
        }
        Walked {
            fns,
            decls,
            classes,
            init: self.init_of(mi),
            globals: self
                .globals_of(mi)
                .into_iter()
                .filter_map(|g| g.init.as_ref())
                .collect(),
        }
    }
}
