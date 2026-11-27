//! FVA 4.3b: a for-in body is a function of its key.
//!
//! HIR lowers `for (k in o) body` to `__obj = o; __keys = ForInKeys(__obj);
//! __n = __keys.length; for (i < __n …) if (guard) { const k = __keys[i]; body }`. Generation makes the
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
    /// The iterated object's local (`__forin_obj`; for a for-of over its
    /// keys, the operand of `Object.keys` / `getOwnPropertyNames`).
    pub obj: u32,
    /// The loop variable.
    pub key: u32,
    /// The for-in's guard (a for-of walk has none).
    pub guard: Option<&'s Expr>,
    /// Which keys the loop visits.
    pub list: KeyList,
    /// `const k = __keys[i]`.
    pub bind: &'s Stmt,
    pub body: &'s [Stmt],
}

/// `stmts` (the `For` body) as a for-in over the keys local `keys` of
/// object local `obj` (`Gen::for_in_keys_of`): `if (guard) { const k =
/// __keys[i]; body }`.
/// What the receiver of an ENUM read `Q[k]` must be (`Gen::enum_scope`).
#[derive(Clone, Debug)]
pub(crate) enum EnumSubject {
    /// ENUM1 (exact): the local `__forin_obj` copies, or the operand of
    /// `Object.keys` whose keys a counted walk binds, never written.
    Local(u32),
    /// F-A (exact): the loop subject path, re-read in the body, with the
    /// gate of its ordinary read (`Gen::path_watch`).
    Path(SubjPath, NodeId),
}

/// A side-effect-free loop subject path `x.a.b` or `this.a.b`: a local or
/// `this`, then one or more static member reads; no call, no computed
/// member. `root` is `None` for `this`. F-A (`Gen::path_watch`) decides
/// whether a body read `Q[k]`, `Q` this same path, reads the iterated
/// object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SubjPath {
    pub(crate) root: Option<u32>,
    pub(crate) members: Vec<String>,
}

impl SubjPath {
    /// Is `e` this path, syntactically?
    pub(crate) fn is(&self, mut e: &Expr) -> bool {
        for m in self.members.iter().rev() {
            match e {
                Expr::PropertyGet {
                    object, property, ..
                } if property == m => e = object,
                _ => return false,
            }
        }
        match (e, self.root) {
            (Expr::LocalGet(id), Some(r)) => *id == r,
            (Expr::This, None) => true,
            _ => false,
        }
    }
}

/// The `SubjPath` of `e`, when it is one.
pub(crate) fn subj_path(mut e: &Expr) -> Option<SubjPath> {
    let mut members = Vec::new();
    let root = loop {
        match e {
            Expr::PropertyGet {
                object, property, ..
            } => {
                members.push(property.clone());
                e = object;
            }
            Expr::LocalGet(id) => break Some(*id),
            Expr::This => break None,
            _ => return None,
        }
    };
    if members.is_empty() {
        return None;
    }
    members.reverse();
    Some(SubjPath { root, members })
}

fn for_in_shape<'s>(
    body: &'s [Stmt],
    keys_of: impl Fn(u32) -> Option<(u32, KeyList)>,
) -> Option<ForIn<'s>> {
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
        init: Some(Expr::IndexGet { object, index, .. } | Expr::RegionOut { object, index, .. }),
        ..
    } = bind
    else {
        return None;
    };
    let (Expr::LocalGet(keys), Expr::LocalGet(_)) = (object.as_ref(), index.as_ref()) else {
        return None;
    };
    let (obj, list) = keys_of(*keys)?;
    if list != KeyList::ForIn {
        return None;
    }
    Some(ForIn {
        obj,
        key: *id,
        guard: Some(condition),
        list,
        bind,
        body: rest,
    })
}

/// E1: the body of a counted for-of walk (`Gen::for_of_walks`) over the
/// keys Array of `Object.keys(o)` / `Object.getOwnPropertyNames(o)`
/// (`Gen::for_in_keys_of`): `const k = arr[i]; body`, `i` the walk's
/// counter. Each iteration binds one element of the fresh Array, one key
/// of `o` (ECMA-262 14.7.5.6 ForIn/OfBodyEvaluation over the Array
/// iterator, 23.1.5.1), so the body is a function of its key as a for-in's.
fn for_of_keys_shape<'s>(
    body: &'s [Stmt],
    keys_of: impl Fn(u32) -> Option<(u32, KeyList)>,
    walked: impl Fn(u32, u32) -> bool,
) -> Option<ForIn<'s>> {
    let (bind, rest) = body.split_first()?;
    let Stmt::Let {
        id,
        init: Some(Expr::IndexGet { object, index, .. } | Expr::RegionOut { object, index, .. }),
        ..
    } = bind
    else {
        return None;
    };
    let (Expr::LocalGet(arr), Expr::LocalGet(i)) = (object.as_ref(), index.as_ref()) else {
        return None;
    };
    let (obj, list) = keys_of(*arr)?;
    if list == KeyList::ForIn || !walked(*arr, *i) {
        return None;
    }
    Some(ForIn {
        obj,
        key: *id,
        guard: None,
        list,
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
        if matches!(s, Stmt::Return(_, _)) {
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
                ..
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
        for w in l.windows(4) {
            let [Stmt::Let {
                id: obj,
                init: Some(src),
                ..
            }, Stmt::Let {
                id: keys,
                init: Some(Expr::ForInKeys(of)),
                ..
            }, Stmt::Let {
                init:
                    Some(Expr::PropertyGet {
                        object: bound_of, ..
                    }),
                ..
            }, Stmt::For { body: fb, .. }] = w
            else {
                continue;
            };
            if !matches!(of.as_ref(), Expr::LocalGet(o) if o == obj)
                || !matches!(bound_of.as_ref(), Expr::LocalGet(k) if k == keys)
            {
                continue;
            }
            let Some(f) = for_in_shape(fb, |k| (k == *keys).then_some((*obj, KeyList::ForIn))) else {
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
        let keys_of = |k| self.for_in_keys_of.get(&(mi, k)).copied();
        let f = for_in_shape(body, keys_of).or_else(|| {
            for_of_keys_shape(body, keys_of, |a, i| self.for_of_walks.contains(&(mi, a, i)))
        })?;
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
        if let Some(g) = f.guard {
            let _ = self.expr(fx, g);
        }
        self.stmt(fx, f.bind);
        let g = self.m.funcs.len() as FnIdx;
        #[cfg(test)]
        let packet_prior = self.m.packet_header(
            g,
            crate::summary::packet::Identity(
                self.modules[mi as usize].0.into(),
                u32::MAX,
                format!("<for-in:{}>", f.key),
            ),
            &[],
        );
        #[cfg(test)]
        let packet_read_start = self.read_sites.len();
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
            #[cfg(test)]
            body_packet: Default::default(),
            module: mi,
            func_id: u32::MAX,
            raise: u32::MAX,
            name: format!("{}:for-in#{g}", self.mods[mi as usize].path_short()),
            params: vec![key],
            rest: None,
            this,
            ret,
            args_all,
            has_arguments: false,
            arguments: None,
            own_test: OwnTestBody::Unknown,
            token: tok,
            ctor_of: None,
            lexical_this: true,
            no_summary: false,
            foreign_params: false,
            gen_proto: None,
            gen_fn_protos: None,
            gen_async: false,
            gen_reyields: false,
            proto: None,
            captures: Vec::new(),
            forwards_super: false,
            parent: fx.f,
            forwarded: false,
            ret_param: None,
        });
        // The enclosing `this`, read as an arrow reads it.
        if f.body.iter().any(crate::gen_call::stmt_reads_this) {
            self.m.edge(fx.this, this);
            self.m.captures.insert((fx.this, g));
        }
        let bfx = Fx {
            guard: None,
            live: None,
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
        #[cfg(test)]
        {
            self.packet_reads(packet_read_start);
            self.m.packet_end(packet_prior, true);
        }
        // A for-of over the keys of local `o`: the keys Array was made from
        // `o` as read at the loop (narrowed there: `p !== Object.prototype`
        // in an enclosing loop's test), so the runs are over that read.
        let on = match f.list {
            KeyList::ForIn => self.local(mi, f.obj),
            _ => self.expr(fx, &Expr::LocalGet(f.obj)),
        };
        let gn = self.tok_node(tok);
        // The template call's key: any String only once a run with every
        // key is made (`Model::for_in_every_key`); a per-key run binds its
        // own constant key.
        let k = self.tmp("forin-key");
        let (call, _) = self.new_call(ThisArg::Undef, vec![k], vec![]);
        self.m.cons(
            gn,
            Cons::ForInBody {
                obj: on,
                call,
                list: f.list,
            },
        );
        // A keys Array a namespace pair made: what a replaced member returns
        // (its generic arm) runs the body with every key.
        let arr = match f.bind {
            Stmt::Let {
                init: Some(Expr::IndexGet { object, .. } | Expr::RegionOut { object, .. }),
                ..
            } => match object.as_ref() {
                Expr::LocalGet(a) => Some(*a),
                _ => None,
            },
            _ => None,
        };
        if let Some(&g) = arr.and_then(|a| self.keys_generic.get(&(mi, a))) {
            let w = self.tmp("forin-replaced-keys");
            let r = self.m.reason("keys of a replaced Object member");
            self.m.cons(
                g,
                Cons::TopIfAny {
                    dst: w,
                    from: w,
                    reason: Some(r),
                },
            );
            self.m.cons(
                gn,
                Cons::ForInBody {
                    obj: w,
                    call,
                    list: f.list,
                },
            );
        }
    }

    /// ENUM1: the for-in of `body` (outlined or a loop) when neither its
    /// key nor the local it iterates is written anywhere: (module, key,
    /// iterated local). The local is the one `__forin_obj` copies (`for (k
    /// in o)` lowers to `__forin_obj = o`), so `o[k]` in the body reads one
    /// of `o`'s enumerable keys.
    ///
    /// F-A: for `for (k in P)` with `P` a side-effect-free path
    /// (`SubjPath`), `P[k]` in the body is that same read until the body
    /// may change the path (`path_watch`).
    ///
    /// ENUM1 over `Object.keys(o)`: the counted walk of the keys Array
    /// (`for_of_keys_shape`, rule E1) binds `k` to one of `o`'s own
    /// enumerable String keys (ECMA-262 20.1.2.18 Object.keys, 7.3.23
    /// EnumerableOwnProperties), so `o[k]` reads one of `o`'s enumerable
    /// keys, as in `for (k in o)` (whose read rule also covers the
    /// inherited ones), while neither `k` nor `o` is written. The subject is
    /// `o` itself: `Object.keys` reads the local, no `__forin_obj` copy.
    /// `Object.getOwnPropertyNames` lists non-enumerable keys too: no rule.
    pub(crate) fn enum_scope(&mut self, fx: &Fx, body: &[Stmt]) -> Option<(u32, u32, EnumSubject)> {
        let mi = fx.module;
        let keys_of = |k| self.for_in_keys_of.get(&(mi, k)).copied();
        let f = match for_in_shape(body, keys_of) {
            Some(f) => f,
            None => {
                let walked = |a, i| self.for_of_walks.contains(&(mi, a, i));
                let f = for_of_keys_shape(body, keys_of, walked)?;
                if f.list != KeyList::OwnKeys {
                    return None;
                }
                let (key, obj) = (f.key, f.obj);
                let w = self.written_locals(mi);
                if w.contains(&key) || w.contains(&obj) {
                    return None;
                }
                return Some((mi, key, EnumSubject::Local(obj)));
            }
        };
        let (key, obj) = (f.key, f.obj);
        let src = self.local_copies.get(&(mi, obj)).copied();
        let path = match src {
            Some(_) => None,
            None => Some(self.for_in_paths.get(&(mi, obj)).cloned()?),
        };
        let w = self.written_locals(mi);
        if w.contains(&key) || w.contains(&obj) {
            return None;
        }
        match (src, path) {
            (Some(src), _) if !w.contains(&src) => Some((mi, key, EnumSubject::Local(src))),
            (None, Some(p)) if p.root.is_none_or(|r| !w.contains(&r)) => {
                let g = self.path_watch(fx, &f, &p);
                if std::env::var_os("PERRY_FACTS_FORIN_PATH_COUNT").is_some() {
                    eprintln!("forin-path: module {mi} key {key} gate {g:?} path {p:?}");
                }
                Some((mi, key, EnumSubject::Path(p, g?)))
            }
            _ => None,
        }
    }

    /// `o[k]` is a read of an enclosing for-in's own key on the binding it
    /// iterates (`enum_scope`): `Cons::IdxLoad`'s `enumerable`.
    pub(crate) fn enum_read(&self, mi: u32, object: &Expr, index: &Expr) -> bool {
        self.enum_subjects(mi, index).any(|s| match s {
            EnumSubject::Local(src) => matches!(object, Expr::LocalGet(l) if l == src),
            EnumSubject::Path(p, _) => p.is(object),
        })
    }

    /// F-A: the gate of `object[index]` when it is the read of an enclosing
    /// for-in's path subject (`EnumSubject::Path`).
    pub(crate) fn enum_gate(&self, mi: u32, object: &Expr, index: &Expr) -> Option<NodeId> {
        self.enum_subjects(mi, index).find_map(|s| match s {
            EnumSubject::Path(p, g) if p.is(object) => Some(*g),
            _ => None,
        })
    }

    /// F-A (exact; replaces the optimistic Rule R): `P[k]` in the body of
    /// `for (k in P)`, `P = r.f1…fn`, `r` an unwritten local or `this`,
    /// reads the object the loop enumerates (ECMA-262 14.7.5.6
    /// ForIn/OfHeadEvaluation evaluates `P` once; 14.7.5.9
    /// EnumerateObjectProperties yields a key only while it names an
    /// enumerable property; `P[k]` re-evaluates `P`) iff no link `fi`
    /// changes between the head and the read: the head and every iteration
    /// before the read run only the body.
    /// - The body runs no other code (`path_body_pure`): no call, `new`,
    ///   coercing operator, iterator or computed key that is not `k` or a
    ///   literal; what remains are reads, stores, deletes and the
    ///   accessors those may run, all watched here.
    /// - No store or delete of the body, under `fi` or an unknown key
    ///   (`__proto__` counts as unknown: it may change what is inherited),
    ///   has a receiver value that is also a value of the holder
    ///   `r.f1…f(i-1)` (`Cons::ForInMeet` on both nodes).
    /// - No read or store of the body, and no link `fi` on its holder, may
    ///   run a program accessor (`Cons::ForInAcc` into a probe; a built-in
    ///   accessor runs no program code).
    /// ⊤ anywhere there opens the gate. The gate only ever opens: the
    /// ordinary read (`Cons::IdxLoad` with `enumerable: false` on the gate)
    /// is added, nothing is removed. `None`: the body is not pure, the read
    /// is the ordinary one.
    fn path_watch(&mut self, fx: &Fx, f: &ForIn<'_>, p: &SubjPath) -> Option<NodeId> {
        if !path_body_pure(f.body, f.key) {
            return None;
        }
        let mi = fx.module;
        // (receiver, key: `None` unknown, 0 read / 1 store / 2 delete)
        let mut sites: Vec<(&Expr, Option<&str>, u8)> = Vec::new();
        let mut bad = false;
        for_each_stmts_expr(f.body, &mut |e| match e {
            Expr::PropertyGet {
                object, property, ..
            } => sites.push((object, Some(property.as_str()), 0)),
            Expr::PropertySet {
                object, property, ..
            } => sites.push((object, Some(property.as_str()), 1)),
            Expr::IndexGet { object, index, .. } | Expr::RegionOut { object, index, .. } => {
                sites.push((object, lit_key(index), 0))
            }
            Expr::IndexSet { object, index, .. } => sites.push((object, lit_key(index), 1)),
            Expr::PutValueSet {
                target,
                key,
                receiver,
                ..
            } => {
                // OrdinarySet (10.1.9.2) runs `target`'s setters and
                // writes the data property on `receiver`.
                sites.push((target, lit_key(key), 1));
                sites.push((receiver, lit_key(key), 1));
            }
            Expr::Delete(t) => match t.as_ref() {
                Expr::PropertyGet {
                    object, property, ..
                } => sites.push((object, Some(property.as_str()), 2)),
                Expr::IndexGet { object, index, .. } | Expr::RegionOut { object, index, .. } => {
                    sites.push((object, lit_key(index), 2))
                }
                _ => bad = true,
            },
            _ => {}
        });
        if bad {
            return None;
        }
        let mut recvs = Vec::with_capacity(sites.len());
        for (o, key, kind) in sites {
            recvs.push((self.path_node(fx, o)?, key, kind));
        }
        let src = self.local(mi, f.obj);
        let gate = self.tmp("forin-gate");
        let probe = self.tmp("forin-acc");
        self.m.cons(
            probe,
            Cons::ForInMeet {
                other: NO_NODE,
                src,
                gate,
            },
        );
        let mut h = match p.root {
            Some(r) => self.local(mi, r),
            None => fx.this,
        };
        let mut holders = Vec::with_capacity(p.members.len());
        for m in &p.members {
            let a = self.m.atom(m);
            holders.push((h, m.as_str()));
            self.m.cons(
                h,
                Cons::ForInAcc {
                    key: Key::Named(a),
                    setter: false,
                    probe,
                },
            );
            h = self.load(h, Key::Named(a));
        }
        for (r, key, kind) in recvs {
            let k = match key {
                Some(s) => Key::Named(self.m.atom(s)),
                None => Key::any(),
            };
            if kind != 2 {
                self.m.cons(
                    r,
                    Cons::ForInAcc {
                        key: k,
                        setter: kind == 1,
                        probe,
                    },
                );
            }
            if kind == 0 {
                continue;
            }
            let key = key.filter(|s| *s != "__proto__");
            for &(hn, f) in &holders {
                if key.is_none_or(|s| s == f) {
                    self.m.cons(
                        r,
                        Cons::ForInMeet {
                            other: hn,
                            src,
                            gate,
                        },
                    );
                    self.m.cons(
                        hn,
                        Cons::ForInMeet {
                            other: r,
                            src,
                            gate,
                        },
                    );
                }
            }
        }
        Some(gate)
    }

    /// F-A: the node of a body receiver that is a local, `this` or a static
    /// member path of one (the same loads the body makes); `None` for any
    /// other receiver.
    fn path_node(&mut self, fx: &Fx, e: &Expr) -> Option<NodeId> {
        Some(match e {
            Expr::LocalGet(id) => self.local(fx.module, *id),
            Expr::This => fx.this,
            Expr::PropertyGet {
                object, property, ..
            } => {
                let o = self.path_node(fx, object)?;
                let a = self.m.atom(property);
                self.load(o, Key::Named(a))
            }
            _ => return None,
        })
    }

    /// The subjects of the enclosing ENUM-scoped for-ins (`enum_scope`)
    /// whose key `index` reads.
    fn enum_subjects<'s>(
        &'s self,
        mi: u32,
        index: &'s Expr,
    ) -> impl Iterator<Item = &'s EnumSubject> + 's {
        self.for_in_reads
            .iter()
            .filter_map(move |(m, key, s)| match index {
                Expr::LocalGet(k) if *m == mi && key == k => Some(s),
                _ => None,
            })
    }

    /// ENUM3: the node of the subject local of the innermost ENUM1-scoped
    /// for-in (`EnumSubject::Local`, exact) whose key `index` reads: a store
    /// under that key stores under the keys the subject enumerates
    /// (`Cons::StoreKeyed`'s `subj`); `NO_NODE` otherwise.
    pub(crate) fn enum_key_subject(&mut self, fx: &Fx, index: &Expr) -> NodeId {
        let src = self
            .enum_subjects(fx.module, index)
            .last()
            .and_then(|s| match s {
                EnumSubject::Local(src) => Some(*src),
                EnumSubject::Path(..) => None,
            });
        match src {
            Some(src) => self.expr(fx, &Expr::LocalGet(src)),
            None => NO_NODE,
        }
    }

    /// SKINROOT11: a store `D[k] = value` with `k` the innermost for-in's
    /// key (a String: ToPropertyKey is pure and gives the same key at the
    /// read and the store) never written, and `value` = `S[k]` or a never
    /// written `const v = S[k]` of the same function (TDZ: its one version
    /// was initialised in this run of the body, with this `k`); `S` a
    /// binding or a member path rooted at one, the root never written. The
    /// store sets D's prototype only under `"__proto__"`, to `S.[[Get]]
    /// ("__proto__")` (B.2.2.1.2, 10.1.8.1): the node of the read
    /// `S["__proto__"]` (`StoreSite::proto_named`). A path re-read here
    /// reads its fields' values (flow-insensitive), a superset of the value
    /// at the read.
    ///
    /// SKINROOT13: the named read has the read kind of the copy's own read
    /// `S[k]`. Where that read is enumerable (`enum_read`: ENUM1, or F-A
    /// for a path subject, with the ordinary read on its gate), for-in yields `k` only if `k` names an
    /// enumerable property on S's chain at that time, and `S[k]` reads that
    /// property unless the body changed it: the named read is the
    /// enumerable read of `"__proto__"` (`Cons::EnumLoad`), which never
    /// finds Object.prototype's `__proto__` accessor (non-enumerable,
    /// 20.1.3.8) or a built-in member. Elsewhere it is the ordinary read,
    /// accessor included (DECISIONS 217).
    pub(crate) fn same_key_proto_src(
        &mut self,
        fx: &Fx,
        key: &Expr,
        value: &Expr,
    ) -> Option<NodeId> {
        let Expr::LocalGet(k) = key else { return None };
        self.for_in_key_of(fx.module, key)?;
        let subj = match value {
            Expr::IndexGet { object, index, .. } | Expr::RegionOut { object, index, .. } if matches!(index.as_ref(), Expr::LocalGet(k2) if k2 == k) => {
                object.as_ref().clone()
            }
            Expr::LocalGet(l) => {
                let (k2, s, f) = self.same_key_reads.get(&(fx.module, *l))?.clone();
                if k2 != *k || f != fx.f || self.written_locals(fx.module).contains(l) {
                    return None;
                }
                s
            }
            _ => return None,
        };
        let root = match &subj {
            Expr::LocalGet(r) => *r,
            e => subj_path(e)?.root?,
        };
        let w = self.written_locals(fx.module);
        if w.contains(k) || w.contains(&root) {
            return None;
        }
        let s = self.expr(fx, &subj);
        let a = self.m.atom("__proto__");
        // A path subject is enumerable only under F-A (`path_watch`): once
        // its gate opens (the body may change the path), the ordinary read
        // of `"__proto__"`, accessor included, too.
        if self.enum_read(fx.module, &subj, key) {
            let dst = self.tmp("enumload");
            self.m.cons(
                s,
                Cons::EnumLoad {
                    key: Key::Named(a),
                    dst,
                },
            );
            if let Some(g) = self.enum_gate(fx.module, &subj, key) {
                self.m.cons(
                    g,
                    Cons::Load {
                        key: Key::Named(a),
                        dst,
                    },
                );
            }
            return Some(dst);
        }
        Some(self.load(s, Key::Named(a)))
    }

    /// Locals written anywhere this solve walks (`=`, `++`).
    pub(crate) fn written_locals(&mut self, mi: u32) -> &crate::pts::FxHashSet<u32> {
        if !self.written.contains_key(&mi) {
            let mut s = crate::pts::FxHashSet::default();
            self.walked(mi as usize).each_expr(&mut |e| match e {
                Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
                    s.insert(*id);
                }
                _ => {}
            });
            self.written.insert(mi, s);
        }
        &self.written[&mi]
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
                    let (Expr::Call { callee, args, .. } | Expr::RegionCall { callee, args, .. }) =
                        e
                    else {
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

/// F-A: a literal string key.
fn lit_key(e: &Expr) -> Option<&str> {
    match e {
        Expr::String(s) => Some(s.as_str()),
        _ => None,
    }
}

/// F-A: the body of a for-in over a path runs no code but its own reads,
/// stores and deletes (and the accessors those may run, `Gen::path_watch`):
/// every expression is a literal, a local, `this`, a member read, store or
/// delete (`PutValueSet` included) whose computed key is the loop key `key` or a string literal, an
/// object or array literal without spread or computed keys, a sequence, a
/// conditional, `typeof`, `void`, `!`, `&&`/`||`/`??` or `===`/`!==` (no
/// ToPrimitive, 7.2.15 IsStrictlyEqual).
fn path_body_pure(body: &[Stmt], key: u32) -> bool {
    use perry_hir::{CompareOp, UnaryOp};
    let ok_index =
        |i: &Expr| matches!(i, Expr::String(_)) || matches!(i, Expr::LocalGet(l) if *l == key);
    let mut ok = true;
    for_each_stmts_expr(body, &mut |e| {
        ok &= match e {
            Expr::IndexGet { index, .. }
            | Expr::RegionOut { index, .. }
            | Expr::IndexSet { index, .. } => ok_index(index),
            Expr::PutValueSet { key: index, .. } => ok_index(index),
            Expr::Undefined
            | Expr::Null
            | Expr::Bool(_)
            | Expr::Number(_)
            | Expr::Integer(_)
            | Expr::String(_)
            | Expr::LocalGet(_)
            | Expr::LocalSet(..)
            | Expr::This
            | Expr::PropertyGet { .. }
            | Expr::PropertySet { .. }
            | Expr::Delete(_)
            | Expr::Conditional { .. }
            | Expr::TypeOf(_)
            | Expr::Void(_)
            | Expr::Logical { .. } => true,
            // An object or array literal runs no program code: its
            // properties are made by CreateDataPropertyOrThrow (13.2.5.5
            // PropertyDefinitionEvaluation, 13.2.4.1 ArrayAccumulation),
            // which runs no setter; its static keys need no ToPropertyKey.
            // Its parts (visited themselves) are checked here like any
            // other expression; a spread (`ObjectSpread`, `ArraySpread`)
            // or computed key is no such literal. `__AnonShape_*` is the
            // HIR class of an object literal (its constructor only defines
            // the fields from its arguments). A sequence runs its parts.
            Expr::Object(_) | Expr::Array(..) | Expr::Sequence(_) => true,
            Expr::New { class_name, .. } => class_name.starts_with("__AnonShape_"),
            Expr::Compare { op, .. } => matches!(op, CompareOp::Eq | CompareOp::Ne),
            Expr::Unary { op, .. } => matches!(op, UnaryOp::Not),
            _ => false,
        };
    });
    ok
}
