//! Per-key flow at the two sinks that preserve a for-in key/value pair.
use crate::gen::{Fx, Gen};
use crate::model::*;
use crate::pts::{FxHashMap, FxHashSet};
use perry_hir::{Expr, Stmt};

pub type Watch = (Tok, NodeId, SiteId, bool, Option<FnIdx>);
#[derive(Default)]
pub struct ForIn {
    pub sources: FxHashMap<Tok, NodeId>,
    pub getter_sources: FxHashMap<FnIdx, NodeId>,
    pub stopped: FxHashMap<Tok, Vec<(Atom, NodeId)>>,
    pub watched_keys: FxHashSet<(Tok, Atom)>,
    pub getters: FxHashMap<(FnIdx, NodeId), FnIdx>,
    pub new_atoms: FxHashSet<Atom>,
    pub any_done: FxHashSet<(NodeId, NodeId, SiteId, bool, Option<FnIdx>)>,
    pub watchers: FxHashMap<Tok, FxHashSet<Watch>>,
    pub done: FxHashSet<(Tok, NodeId, Key, SiteId, bool, Option<FnIdx>)>,
    pub values: FxHashMap<(Tok, Key, Option<FnIdx>), NodeId>,
}

impl Model {
    pub(crate) fn forin_source(
        &mut self,
        src: Tok,
        dst: NodeId,
        site: SiteId,
        accessor: bool,
        getter: Option<FnIdx>,
    ) {
        self.forin_holder(src, src, dst, site, accessor, getter);
        if src != TOP && !self.is_escaped(src) {
            let cp = self.cp(src);
            self.cons(
                cp,
                Cons::ForInProto {
                    origin: src,
                    dst,
                    site,
                    accessor,
                    getter,
                },
            );
        }
    }
    pub(crate) fn forin_holder(
        &mut self,
        holder: Tok,
        origin: Tok,
        dst: NodeId,
        site: SiteId,
        accessor: bool,
        getter: Option<FnIdx>,
    ) {
        if !self
            .forin
            .watchers
            .entry(holder)
            .or_default()
            .insert((origin, dst, site, accessor, getter))
        {
            return;
        }
        if holder == TOP || self.is_escaped(holder) {
            let value = self.top_tok_node;
            self.forin_install(origin, dst, Key::Any, value, site, accessor, getter);
            return;
        }
        let mut keys = self.field_keys.get(&holder).cloned().unwrap_or_default();
        if let Some(ak) = self.accessor_keys.get(&holder) {
            keys.extend(ak.iter().copied());
        }
        keys.sort_unstable();
        keys.dedup();
        for a in keys {
            self.forin_key(holder, a);
        }
        // Unknown-key writes are a real unknown-key consumer. They remain
        // conservative, without unioning the separately known named fields.
        let any = self.field_any(holder);
        self.cons(
            any,
            Cons::ForInAny {
                dst,
                site,
                accessor,
                getter,
            },
        );
    }
    pub(crate) fn forin_key(&mut self, holder: Tok, key: Atom) {
        if !self.forin.watchers.contains_key(&holder) {
            return;
        }
        let birth = self
            .tmpl_of(holder)
            .is_some_and(|tm| self.tmpls[tm as usize].birth_keys.contains(&key));
        let accessor = self
            .accessor_keys
            .get(&holder)
            .is_some_and(|keys| keys.contains(&key));
        if birth || accessor {
            self.forin_key_ready(holder, key);
        }
        if let Some(&field) = self.fields.get(&(holder, key)) {
            if self.forin.watched_keys.insert((holder, key)) {
                self.cons(field, Cons::ForInKey { holder, key });
            } else if !self.nodes[field as usize].pts.is_empty()
                || self.nodes[field as usize].prim != 0
            {
                self.forin_key_ready(holder, key);
            }
        }
    }
    pub(crate) fn forin_key_ready(&mut self, holder: Tok, key: Atom) {
        let watchers = self
            .forin
            .watchers
            .get(&holder)
            .cloned()
            .unwrap_or_default();
        for (origin, dst, site, accessor, getter) in watchers {
            if self.is_escaped(holder) || self.is_escaped(origin) {
                continue;
            }
            self.forin_named(origin, dst, key, site, accessor, getter);
        }
    }
    fn forin_named(
        &mut self,
        src: Tok,
        dst: NodeId,
        a: Atom,
        site: SiteId,
        accessor: bool,
        getter: Option<FnIdx>,
    ) {
        let key = Key::Named(a);
        if !self
            .forin
            .done
            .insert((src, dst, key, site, accessor, getter))
        {
            return;
        }
        let value = if let Some(&n) = self.forin.values.get(&(src, key, getter)) {
            n
        } else {
            let n = self.node_l("forin-value");
            self.forin.values.insert((src, key, getter), n);
            let name = self.key_name(key);
            self.rel(|| {
                format!(
                    "forinvalue {n} {src} {name} {}",
                    getter.map(|f| f as i64).unwrap_or(-1)
                )
            });
            if let Some(source) = getter.and_then(|f| self.forin.getter_sources.get(&f).copied()) {
                self.cons(source, Cons::ForInRead { key: a, dst: n });
            } else {
                self.forin_read(src, a, n);
            }
            n
        };
        self.forin_install(src, dst, key, value, site, accessor, getter);
    }
    pub(crate) fn forin_read(&mut self, src: Tok, key: Atom, dst: NodeId) {
        // Literal birth properties are own data properties. Their values
        // cannot come from a prototype while that definition stands.
        let own = self.tmpl_of(src).is_some_and(|tm| {
            let template = &self.tmpls[tm as usize];
            matches!(
                template.kind,
                TmplKind::Lit { .. }
                    | TmplKind::Class {
                        literal_shape: true,
                        ..
                    }
            ) && template.definite.contains(&key)
                && !template.accessor_keys.contains(&key)
        }) && !self.hard.contains_key(&src)
            && !self.accessors.contains_key(&(src, key));
        if own {
            let field = self.field(src, key);
            self.edge(field, dst);
            self.forin.stopped.entry(src).or_default().push((key, dst));
        } else {
            self.forin_full_read(src, key, dst);
        }
    }
    fn forin_full_read(&mut self, src: Tok, key: Atom, dst: NodeId) {
        let recv = if let Some(&n) = self.forin.sources.get(&src) {
            n
        } else {
            let n = self.node_l("forin-receiver");
            self.add_tok(n, src);
            self.forin.sources.insert(src, n);
            n
        };
        self.cons(
            recv,
            Cons::Load {
                key: Key::Named(key),
                dst,
            },
        );
    }
    pub(crate) fn resume_forin_reads(&mut self, src: Tok) {
        for (key, dst) in self.forin.stopped.remove(&src).unwrap_or_default() {
            self.forin_full_read(src, key, dst);
        }
    }
    pub(crate) fn forin_install(
        &mut self,
        _src: Tok,
        dst: NodeId,
        key: Key,
        value: NodeId,
        site: SiteId,
        accessor: bool,
        getter: Option<FnIdx>,
    ) {
        if !accessor {
            self.cons(
                dst,
                Cons::Store {
                    key,
                    src: value,
                    site,
                },
            );
            return;
        }
        let value = if let Some(f) = getter {
            let copy = if let Some(&copy) = self.forin.getters.get(&(f, value)) {
                copy
            } else {
                let template = self.funcs[f as usize].clone();
                let copy = self.funcs.len() as FnIdx;
                let token = self.tok(TokKind::Func(copy));
                let this = self.node_l("forin-getter-this");
                let ret = self.node_l("forin-getter-ret");
                let args_all = self.node_l("forin-getter-args");
                self.funcs.push(FuncInfo {
                    token,
                    this,
                    ret,
                    args_all,
                    ..template
                });
                self.edge(value, ret);
                self.forin.getters.insert((f, value), copy);
                self.rel(|| format!("foringetter {f} {value} {copy}"));
                copy
            };
            let token = self.funcs[copy as usize].token;
            let n = self.node_l("forin-getter");
            self.add_tok(n, token);
            n
        } else {
            value
        };
        let a = match key {
            Key::Named(a) => a,
            Key::Any => self.atom(crate::STAR_KEY),
        };
        if self.accessor_atoms.insert(a) {
            self.forin.new_atoms.insert(a);
        }
        self.cons(
            value,
            Cons::Accessor {
                holder: dst,
                key: a,
                setter: false,
            },
        );
        // Missing descriptor properties inherit from Object.prototype.
        // Its data-valued `set` and `value` may be written later. Accessor
        // mutations there already disable facts globally (E23).
        let proto = self.tok(TokKind::ObjectProto);
        let set = self.atom("set");
        let set = self.field(proto, set);
        self.cons(
            set,
            Cons::Accessor {
                holder: dst,
                key: a,
                setter: true,
            },
        );
        let val = self.atom("value");
        let val = self.field(proto, val);
        self.cons(
            dst,
            Cons::Store {
                key,
                src: val,
                site,
            },
        );
    }

    /// Named accessor keys become known as source objects reach the helper.
    /// Replay only accessor walks skipped before those keys were known.
    pub(crate) fn replay_forin_accessors(&mut self) -> bool {
        let atoms = std::mem::take(&mut self.forin.new_atoms);
        if atoms.is_empty() {
            return false;
        }
        let mut todo = Vec::new();
        for (n, node) in self.nodes.iter().enumerate() {
            for c in &node.cons {
                let pair = match *c {
                    Cons::Load { key, dst } => Some((key, ChainAct::Load(dst))),
                    Cons::Store { key, src, .. } => Some((key, ChainAct::Store(src))),
                    Cons::MCall { key, call } => Some((key, ChainAct::Call(call))),
                    Cons::IdxLoad { dst, .. } => Some((Key::Any, ChainAct::Load(dst))),
                    _ => None,
                };
                if let Some((key, act)) = pair {
                    if matches!(key, Key::Any) || matches!(key, Key::Named(a) if atoms.contains(&a))
                    {
                        todo.push((n as NodeId, key, act));
                    }
                }
            }
        }
        for (n, key, act) in todo {
            let toks = self.nodes[n as usize].pts.to_sorted();
            self.work += toks.len() as u64;
            if self.max_work != 0 && self.work > self.max_work {
                self.aborted = true;
                return false;
            }
            for t in toks {
                self.accessor_walk(t, t, key, act);
            }
        }
        true
    }
}

impl<'a> Gen<'a> {
    pub(crate) fn prepare_forin_captures(&mut self) {
        for (mi, (_, module)) in self.modules.clone().iter().enumerate() {
            let mut captures = FxHashSet::default();
            mutable_captures_stmts(&module.init, &mut captures);
            for global in &module.globals {
                if let Some(e) = &global.init {
                    mutable_captures_expr(e, &mut captures);
                }
            }
            for f in &module.functions {
                mutable_captures_fn(f, &mut captures);
            }
            for c in &module.classes {
                for f in c
                    .constructor
                    .iter()
                    .chain(c.methods.iter())
                    .chain(c.static_methods.iter())
                    .chain(c.getters.iter().chain(&c.setters).map(|(_, f)| f))
                {
                    mutable_captures_fn(f, &mut captures);
                }
                for field in c.fields.iter().chain(&c.static_fields) {
                    if let Some(e) = &field.init {
                        mutable_captures_expr(e, &mut captures);
                    }
                }
                for member in &c.computed_members {
                    mutable_captures_fn(&member.function, &mut captures);
                }
            }
            self.forin_mutable_captures
                .extend(captures.into_iter().map(|id| (mi as u32, id)));
        }
    }
    pub(crate) fn forin_binding(&mut self, fx: &Fx, id: u32, e: &Expr) {
        if let Expr::LocalGet(src) = e {
            self.forin_aliases.insert((fx.module, id), *src);
        }
        if let Expr::ForInKeys(src) = e {
            if let Expr::LocalGet(src) = src.as_ref() {
                // Follow the compiler's one snapshot spill, not arbitrary
                // user aliases whose bindings may have changed.
                let src = self
                    .forin_aliases
                    .get(&(fx.module, *src))
                    .copied()
                    .unwrap_or(*src);
                self.forin_keys.insert((fx.module, id), src);
            }
        }
    }
    pub(crate) fn enter_forin(&mut self, fx: &Fx, body: &[Stmt]) -> Vec<(u32, Option<u32>)> {
        let mut bindings = Vec::new();
        find_bindings(body, &mut bindings);
        let mut saved = Vec::new();
        let mut declarations = FxHashMap::default();
        declaration_counts(body, &mut declarations);
        for (key, keys) in bindings {
            let Some(&src) = self.forin_keys.get(&(fx.module, keys)) else {
                continue;
            };
            let mut writes = FxHashSet::default();
            writes_in(body, &mut writes);
            if writes.contains(&src)
                || writes.contains(&key)
                || declarations.contains_key(&src)
                || declarations.get(&key).copied().unwrap_or(0) > 1
                || self.forin_mutable_captures.contains(&(fx.module, src))
                || self.forin_mutable_captures.contains(&(fx.module, key))
            {
                continue;
            }
            let old = self.forin_active.insert((fx.f, key), src);
            saved.push((key, old));
        }
        saved
    }
    pub(crate) fn leave_forin(&mut self, fx: &Fx, saved: Vec<(u32, Option<u32>)>) {
        for (key, old) in saved {
            if let Some(old) = old {
                self.forin_active.insert((fx.f, key), old);
            } else {
                self.forin_active.remove(&(fx.f, key));
            }
        }
    }
    fn paired_read<'b>(&self, fx: &Fx, key: &Expr, value: &'b Expr) -> Option<&'b Expr> {
        let Expr::LocalGet(k) = key else {
            return None;
        };
        let &src = self.forin_active.get(&(fx.f, *k))?;
        let Expr::IndexGet { object, index } = value else {
            return None;
        };
        let (Expr::LocalGet(o), Expr::LocalGet(i)) = (object.as_ref(), index.as_ref()) else {
            return None;
        };
        (*i == *k && *o == src).then_some(object)
    }
    /// Only statement sinks consume the symbolic pair. An assignment value
    /// used elsewhere must retain the ordinary unknown-key load.
    pub(crate) fn correlated_stmt(&mut self, fx: &Fx, e: &Expr) -> bool {
        let copy = match e {
            Expr::IndexSet {
                object,
                index,
                value,
            } => Some((object, index, value)),
            Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            } if matches!((target.as_ref(), receiver.as_ref()), (Expr::LocalGet(a), Expr::LocalGet(b)) if a == b) => {
                Some((target, key, value))
            }
            _ => None,
        };
        if let Some((object, index, value)) = copy {
            if let Some(src) = self.paired_read(fx, index, value).cloned() {
                let dst = self.expr(fx, object);
                let key = self.expr(fx, index);
                let r = self.m.reason("ToPropertyKey");
                self.m.cons(key, Cons::Implicit { reason: r });
                let src = self.expr(fx, &src);
                let site = self.store_site(fx.module, fx.f, "for-in copy".into(), None, false);
                self.m.cons(
                    src,
                    Cons::ForIn {
                        dst,
                        site,
                        accessor: false,
                        getter: None,
                    },
                );
                return true;
            }
        }
        let (target, key, desc) = match e {
            Expr::ObjectDefineProperty(t, k, d) => (t, k, d),
            Expr::ReflectDefineProperty {
                target,
                key,
                descriptor,
            } => (target, key, descriptor),
            _ => return false,
        };
        let props: Vec<(String, Expr)> = match desc.as_ref() {
            Expr::Object(props) => props.clone(),
            Expr::New {
                class_name, args, ..
            } => {
                let Some(class) = self.modules[fx.module as usize]
                    .1
                    .classes
                    .iter()
                    .find(|c| c.name == *class_name && c.is_literal_shape())
                else {
                    return false;
                };
                if class.fields.len() != args.len() {
                    return false;
                }
                class
                    .fields
                    .iter()
                    .zip(args)
                    .map(|(f, e)| (f.name.clone(), e.clone()))
                    .collect()
            }
            _ => return false,
        };
        let Some((_, get)) = props.iter().find(|(k, _)| k == "get") else {
            return false;
        };
        if props.iter().filter(|(k, _)| k == "get").count() != 1
            || props
                .iter()
                .any(|(k, _)| matches!(k.as_str(), "set" | "value" | "writable" | "__proto__"))
        {
            return false;
        }
        let (src, getter) = if let Some(src) = self.paired_read(fx, key, get) {
            (src.clone(), None)
        } else if let Expr::Closure {
            func_id,
            params,
            body,
            is_arrow,
            is_async,
            is_generator,
            ..
        } = get
        {
            if !params.is_empty() || *is_async || *is_generator {
                return false;
            }
            let [Stmt::Return(Some(read))] = body.as_slice() else {
                return false;
            };
            let Some(src) = self.paired_read(fx, key, read).cloned() else {
                return false;
            };
            let f = self.register_fn(
                fx.module,
                *func_id,
                "<for-in getter>",
                params,
                *is_arrow,
                true,
            );
            (src, Some(f))
        } else {
            return false;
        };
        let dst = self.expr(fx, target);
        let kn = self.expr(fx, key);
        let r = self.m.reason("E5 Object.defineProperty");
        self.m.cons(kn, Cons::Implicit { reason: r });
        let src = self.expr(fx, &src);
        for (name, value) in &props {
            if name != "get" {
                let _ = self.expr(fx, value);
            }
        }
        self.m.cons(
            dst,
            Cons::Hard {
                kind: Hard::E5DefineProperty,
                reason: r,
            },
        );
        let site = self.store_site(fx.module, fx.f, "for-in accessor".into(), None, false);
        if let Some(f) = getter {
            let recv = if let Some(&n) = self.m.forin.getter_sources.get(&f) {
                n
            } else {
                let n = self.m.node_l("forin-getter-source");
                self.m.forin.getter_sources.insert(f, n);
                self.m.rel(|| format!("foringettersource {f} {n}"));
                n
            };
            self.m.edge(src, recv);
        }
        self.m.cons(
            src,
            Cons::ForIn {
                dst,
                site,
                accessor: true,
                getter,
            },
        );
        true
    }
}
fn find_bindings(body: &[Stmt], out: &mut Vec<(u32, u32)>) {
    for s in body {
        match s {
            Stmt::Let {
                id,
                init: Some(Expr::IndexGet { object, .. }),
                ..
            } => {
                if let Expr::LocalGet(keys) = object.as_ref() {
                    out.push((*id, *keys));
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                find_bindings(then_branch, out);
                if let Some(b) = else_branch {
                    find_bindings(b, out);
                }
            }
            _ => {}
        }
    }
}
fn writes_expr(e: &Expr, out: &mut FxHashSet<u32>) {
    match e {
        Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
            out.insert(*id);
        }
        Expr::Closure { body, .. } => writes_in(body, out),
        Expr::WithSet { fallback, .. } => {
            if let perry_hir::WithSetFallback::Local(id)
            | perry_hir::WithSetFallback::SloppyImplicit(id) = fallback
            {
                out.insert(*id);
            }
        }
        _ => {}
    }
    perry_hir::walker::walk_expr_children(e, &mut |c| writes_expr(c, out));
}
fn writes_in(body: &[Stmt], out: &mut FxHashSet<u32>) {
    for s in body {
        perry_hir::walker::stmt_any_expr(s, &mut |e| {
            writes_expr(e, out);
            false
        });
    }
}

fn mutable_captures_fn(f: &perry_hir::Function, out: &mut FxHashSet<u32>) {
    for p in &f.params {
        if let Some(e) = &p.default {
            mutable_captures_expr(e, out);
        }
    }
    mutable_captures_stmts(&f.body, out);
}
fn mutable_captures_expr(e: &Expr, out: &mut FxHashSet<u32>) {
    if let Expr::Closure {
        mutable_captures,
        body,
        ..
    } = e
    {
        out.extend(mutable_captures.iter().copied());
        mutable_captures_stmts(body, out);
    }
    perry_hir::walker::walk_expr_children(e, &mut |e| mutable_captures_expr(e, out));
}
fn mutable_captures_stmts(body: &[Stmt], out: &mut FxHashSet<u32>) {
    for s in body {
        perry_hir::walker::stmt_any_expr(s, &mut |e| {
            mutable_captures_expr(e, out);
            false
        });
    }
}

fn declaration_counts(body: &[Stmt], counts: &mut FxHashMap<u32, usize>) {
    fn stmt(s: &Stmt, counts: &mut FxHashMap<u32, usize>) {
        match s {
            Stmt::Let { id, .. } => {
                *counts.entry(*id).or_default() += 1;
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                declaration_counts(then_branch, counts);
                if let Some(body) = else_branch {
                    declaration_counts(body, counts);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                declaration_counts(body, counts)
            }
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    stmt(init, counts);
                }
                declaration_counts(body, counts);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                declaration_counts(body, counts);
                if let Some(catch) = catch {
                    if let Some((id, _)) = &catch.param {
                        *counts.entry(*id).or_default() += 1;
                    }
                    declaration_counts(&catch.body, counts);
                }
                if let Some(body) = finally {
                    declaration_counts(body, counts);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    declaration_counts(&case.body, counts);
                }
            }
            Stmt::Labeled { body, .. } => stmt(body, counts),
            Stmt::Expr(_)
            | Stmt::Throw(_)
            | Stmt::Return(_)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
        }
    }
    for s in body {
        stmt(s, counts);
    }
}
