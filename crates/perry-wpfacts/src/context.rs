//! Bounded, selective 1-CFA. HIR bodies are replayed into separate local
//! nodes; captured bindings and runtime function identity remain shared.
use crate::gen::{Fx, Gen};
use crate::model::*;
use crate::pts::{FxHashMap, FxHashSet};
use perry_hir::{Expr, Param, Stmt};

pub(crate) const NODE_BUDGET: usize = 256;
pub(crate) const COPY_BUDGET: usize = 64;

#[derive(Default)]
pub struct Contexts {
    pub origins: FxHashMap<FnIdx, FnIdx>,
    pub ready: bool,
    pub eligible: FxHashSet<FnIdx>,
    pub fresh: FxHashSet<CallId>,
    pub owners: FxHashMap<CallId, FnIdx>,
    pub sites: FxHashMap<CallId, CallId>,
    pub graph: FxHashMap<FnIdx, FxHashSet<FnIdx>>,
    pub graph_dirty: bool,
    pub recursive: FxHashSet<FnIdx>,
    pub copies: FxHashMap<(FnIdx, CallId), Option<FnIdx>>,
    pub counts: FxHashMap<FnIdx, usize>,
    pub waiting: FxHashMap<(FnIdx, CallId), Vec<CallId>>,
    pub requests: Vec<(FnIdx, CallId)>,
    pub deferred: Vec<(CallId, Tok)>,
    pub bindings: FxHashMap<(CallId, FnIdx), FnIdx>,
}

#[derive(Clone)]
pub(crate) struct Body {
    pub fx: Fx,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    pub locals: FxHashSet<u32>,
    pub closures: FxHashSet<u32>,
    pub captures: Vec<(u32, NodeId)>,
}

impl Model {
    /// None means the generator must first emit a requested body copy.
    pub(crate) fn context_target(&mut self, call: CallId, f: FnIdx) -> Option<FnIdx> {
        let canonical = self.contexts.origins.get(&f).copied().unwrap_or(f);
        if let Some(&owner) = self.contexts.owners.get(&call) {
            if self
                .contexts
                .graph
                .entry(owner)
                .or_default()
                .insert(canonical)
            {
                self.contexts.graph_dirty = true;
            }
        }
        if !self.contexts.ready {
            // Keep ordinary calls and constructors online. Delaying their
            // bindings until generation ends postpones widening behind a
            // much larger graph and makes unrelated solver relays explode.
            if !self.contexts.eligible.contains(&f)
                || !self.contexts.fresh.contains(&call)
                || self.protocol_calls.contains(&call)
            {
                self.contexts.bindings.insert((call, f), f);
                return Some(f);
            }
            self.contexts
                .deferred
                .push((call, self.funcs[f as usize].token));
            return None;
        }
        if !self.contexts.fresh.contains(&call)
            || !self.contexts.eligible.contains(&f)
            || self.contexts.recursive.contains(&canonical)
            || self.protocol_calls.contains(&call)
        {
            self.contexts.bindings.insert((call, f), f);
            return Some(f);
        }
        let site = self.contexts.sites.get(&call).copied().unwrap_or(call);
        let key = (f, site);
        if let Some(&copy) = self.contexts.copies.get(&key) {
            if let Some(copy) = copy {
                self.contexts.bindings.insert((call, f), copy);
                return Some(copy);
            }
        } else {
            let count = self.contexts.counts.entry(canonical).or_default();
            if *count >= COPY_BUDGET {
                self.contexts.bindings.insert((call, f), f);
                return Some(f);
            }
            *count += 1;
            self.contexts.copies.insert(key, None);
            self.contexts.requests.push(key);
        }
        let waiting = self.contexts.waiting.entry(key).or_default();
        if !waiting.contains(&call) {
            waiting.push(call);
        }
        None
    }

    /// Recursion is a property of the resolved call graph, not the call
    /// stack used to emit copies. Batch SCC discovery between solver waves.
    pub(crate) fn exclude_recursive_contexts(&mut self) {
        if !std::mem::take(&mut self.contexts.graph_dirty) {
            return;
        }
        let n = self.funcs.len();
        let mut reverse = vec![Vec::new(); n];
        for (&src, targets) in &self.contexts.graph {
            for &dst in targets {
                reverse[dst as usize].push(src);
            }
        }
        let mut seen = vec![false; n];
        let mut order = Vec::new();
        for root in 0..n as FnIdx {
            let mut stack = vec![(root, false)];
            while let Some((f, leave)) = stack.pop() {
                if leave {
                    order.push(f);
                    continue;
                }
                if std::mem::replace(&mut seen[f as usize], true) {
                    continue;
                }
                stack.push((f, true));
                if let Some(targets) = self.contexts.graph.get(&f) {
                    for &target in targets {
                        if !seen[target as usize] {
                            stack.push((target, false));
                        }
                    }
                }
            }
        }
        seen.fill(false);
        for root in order.into_iter().rev() {
            if seen[root as usize] {
                continue;
            }
            let mut group = Vec::new();
            let mut stack = vec![root];
            while let Some(f) = stack.pop() {
                if std::mem::replace(&mut seen[f as usize], true) {
                    continue;
                }
                group.push(f);
                stack.extend(reverse[f as usize].iter().copied());
            }
            if group.len() > 1
                || self
                    .contexts
                    .graph
                    .get(&root)
                    .is_some_and(|next| next.contains(&root))
            {
                for f in group {
                    self.demote_context(f);
                }
            }
        }
    }

    fn demote_context(&mut self, f: FnIdx) {
        if !self.contexts.recursive.insert(f) {
            return;
        }
        let copies: Vec<(FnIdx, FnIdx)> = self
            .contexts
            .copies
            .iter()
            .filter_map(|(&(orig, _), &copy)| {
                let canonical = self.contexts.origins.get(&orig).copied().unwrap_or(orig);
                (canonical == f)
                    .then_some(copy.map(|copy| (orig, copy)))
                    .flatten()
            })
            .collect();
        for (orig, copy) in copies {
            let original = orig;
            let orig = self.funcs[orig as usize].clone();
            let clone = self.funcs[copy as usize].clone();
            // Previously emitted copies can keep running, but their calls
            // now participate in the merged recursive analysis as well.
            for (&src, &dst) in clone.params.iter().zip(&orig.params) {
                self.edge(src, dst);
            }
            self.edge(clone.this, orig.this);
            self.edge(clone.args_all, orig.args_all);
            if let (Some(a), Some(b)) = (clone.rest, orig.rest) {
                self.edge(a, b);
            }
            self.edge(orig.ret, clone.ret);
            self.rel(|| format!("contextmerge {original} {copy}"));
        }
    }
}

impl<'a> Gen<'a> {
    pub(crate) fn remember_body(&mut self, fx: &Fx, params: &[Param], body: &[Stmt]) {
        if fx.class.is_some() || fx.ctor_of.is_some() || self.context_bodies.contains_key(&fx.f) {
            return;
        }
        let mut scan = Scan::default();
        for p in params {
            scan.locals.insert(p.id);
        }
        for p in params {
            if let Some(default) = &p.default {
                scan.expr(default);
            }
        }
        scan.stmts(body);
        if scan.nodes > NODE_BUDGET {
            return;
        }
        let mut refs = Vec::new();
        let mut visited = std::collections::HashSet::new();
        for p in params {
            if let Some(default) = &p.default {
                perry_hir::analysis::collect_local_refs_expr(default, &mut refs, &mut visited);
            }
        }
        for stmt in body {
            perry_hir::analysis::collect_local_refs_stmt(stmt, &mut refs, &mut visited);
        }
        scan.refs.extend(refs);
        let captures = scan
            .refs
            .iter()
            .filter(|id| !scan.locals.contains(id))
            .filter_map(|id| {
                self.mods[fx.module as usize]
                    .locals
                    .get(id)
                    .map(|&n| (*id, n))
            })
            .collect();
        self.context_bodies.insert(
            fx.f,
            Body {
                fx: fx.clone(),
                params: params.to_vec(),
                body: body.to_vec(),
                locals: scan.locals,
                closures: scan.closures,
                captures,
            },
        );
        self.m.contexts.eligible.insert(fx.f);
    }

    pub(crate) fn solve_contexts(&mut self) {
        self.m.contexts.ready = true;
        let deferred = std::mem::take(&mut self.m.contexts.deferred);
        for (call, tok) in deferred {
            self.m.bind_args(call, tok);
        }
        loop {
            self.m.exclude_recursive_contexts();
            self.m.solve();
            if self.m.aborted {
                break;
            }
            if self.m.contexts.graph_dirty {
                self.m.exclude_recursive_contexts();
                if !self.m.wl.is_empty() {
                    continue;
                }
            }
            if self.m.aborted || self.m.contexts.requests.is_empty() {
                break;
            }
            let requests = std::mem::take(&mut self.m.contexts.requests);
            for (f, site) in requests {
                let canonical = self.m.contexts.origins.get(&f).copied().unwrap_or(f);
                let copy = if self.m.contexts.recursive.contains(&canonical) {
                    f
                } else {
                    self.copy_body(f)
                };
                self.m.contexts.copies.insert((f, site), Some(copy));
                let waiting = self
                    .m
                    .contexts
                    .waiting
                    .remove(&(f, site))
                    .unwrap_or_default();
                let tok = self.m.funcs[f as usize].token;
                for call in waiting {
                    self.m.bind_args(call, tok);
                }
            }
        }
    }

    fn copy_body(&mut self, f: FnIdx) -> FnIdx {
        let body = self.context_bodies[&f].clone();
        let mi = body.fx.module;
        let original = self.m.funcs[f as usize].clone();
        let mut saved = Vec::new();
        for &id in &body.locals {
            saved.push((id, self.mods[mi as usize].locals.remove(&id)));
        }
        for &(id, node) in &body.captures {
            saved.push((id, self.mods[mi as usize].locals.insert(id, node)));
        }
        let mut functions = Vec::new();
        for &id in &body.closures {
            functions.push((id, self.mods[mi as usize].funcs.remove(&id)));
        }
        let root = self.mods[mi as usize].funcs.remove(&original.func_id);
        let phase = self.m.rel_phase;
        self.m.rel_phase = b'G';
        let copy = self.register_fn(
            mi,
            original.func_id,
            &original.name,
            &body.params,
            original.lexical_this,
            true,
        );
        // A body copy is not a new runtime function. Self references and
        // function properties continue to name the original function value.
        if let Some(root) = root {
            self.mods[mi as usize].funcs.insert(original.func_id, root);
        }
        let info = self.m.funcs[copy as usize].clone();
        let fx = Fx {
            f: copy,
            this: info.this,
            ret: info.ret,
            ..body.fx
        };
        if original.lexical_this {
            self.m.edge(original.this, info.this);
        }
        if fx.fn_ctor {
            self.note_fn_phase(
                copy,
                crate::ctor_phase::analyze_closure(&body.params, &body.body),
            );
        }
        self.params(&fx, &body.params);
        self.stmts(&fx, &body.body);
        for id in &body.locals {
            if let Some(&n) = self.mods[mi as usize].locals.get(id) {
                self.context_locals.entry((mi, *id)).or_default().push(n);
            }
        }
        self.m.rel_phase = phase;
        for (id, old) in saved.into_iter().rev() {
            if let Some(n) = old {
                self.mods[mi as usize].locals.insert(id, n);
            } else {
                self.mods[mi as usize].locals.remove(&id);
            }
        }
        for (id, old) in functions {
            if let Some(n) = old {
                self.mods[mi as usize].funcs.insert(id, n);
            } else {
                self.mods[mi as usize].funcs.remove(&id);
            }
        }
        copy
    }

    /// A native body is emitted once: consumers must see the union of
    /// every analysis context at its source site, including the merged one.
    pub(crate) fn merge_context_sites(&mut self) {
        let mut groups =
            FxHashMap::<(u32, u32, Atom, bool), (crate::gen::ReadSite, Vec<NodeId>)>::default();
        for s in std::mem::take(&mut self.read_sites) {
            let entry = groups
                .entry((s.module, s.byte_offset, s.key, s.is_call))
                .or_insert_with(|| (s.clone(), Vec::new()));
            if !entry.1.contains(&s.recv) {
                entry.1.push(s.recv);
            }
        }
        let phase = self.m.rel_phase;
        self.m.rel_phase = b'G';
        let mut groups: Vec<_> = groups.into_iter().collect();
        groups.sort_by_key(|(key, _)| *key);
        for (_, (mut site, nodes)) in groups {
            if nodes.len() > 1 {
                let recv = self.m.node_l("context-site");
                self.m.nodes[recv as usize].internal = true;
                for n in nodes {
                    self.m.edge(n, recv);
                }
                site.recv = recv;
            }
            self.read_sites.push(site);
        }
        self.m.rel_phase = phase;
        self.m.solve();
    }
}

#[derive(Default)]
struct Scan {
    nodes: usize,
    locals: FxHashSet<u32>,
    closures: FxHashSet<u32>,
    refs: FxHashSet<u32>,
}
impl Scan {
    fn expr(&mut self, e: &Expr) {
        self.nodes += 1;
        if self.nodes > NODE_BUDGET {
            return;
        }
        match e {
            Expr::LocalGet(id) | Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
                self.refs.insert(*id);
            }
            Expr::ScopedTemp { id, .. } => {
                self.locals.insert(*id);
            }
            Expr::Closure {
                func_id,
                params,
                body,
                is_async,
                is_generator,
                ..
            } => {
                if *is_async || *is_generator {
                    self.nodes = NODE_BUDGET + 1;
                    return;
                }
                self.closures.insert(*func_id);
                for p in params {
                    self.locals.insert(p.id);
                }
                self.stmts(body);
            }
            _ => {}
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| self.expr(c));
    }
    fn stmts(&mut self, body: &[Stmt]) {
        for s in body {
            self.stmt(s);
        }
    }
    fn stmt(&mut self, s: &Stmt) {
        self.nodes += 1;
        if self.nodes > NODE_BUDGET {
            return;
        }
        match s {
            Stmt::Let { id, init, .. } => {
                self.locals.insert(*id);
                if let Some(e) = init {
                    self.expr(e);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => self.expr(e),
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.expr(e);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition);
                self.stmts(then_branch);
                if let Some(b) = else_branch {
                    self.stmts(b);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                self.expr(condition);
                self.stmts(body);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(s) = init {
                    self.stmt(s);
                }
                if let Some(e) = condition {
                    self.expr(e);
                }
                if let Some(e) = update {
                    self.expr(e);
                }
                self.stmts(body);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.stmts(body);
                if let Some(c) = catch {
                    if let Some((id, _)) = &c.param {
                        self.locals.insert(*id);
                    }
                    self.stmts(&c.body);
                }
                if let Some(b) = finally {
                    self.stmts(b);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant);
                for c in cases {
                    if let Some(e) = &c.test {
                        self.expr(e);
                    }
                    self.stmts(&c.body);
                }
            }
            Stmt::Labeled { body, .. } => self.stmt(body),
            Stmt::PreallocateTdzBoxes(ids) => {
                self.locals.extend(ids);
            }
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
        }
    }
}

pub(crate) fn fresh_arg(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Object(_)
            | Expr::ObjectSpread { .. }
            | Expr::Array(_)
            | Expr::ArraySpread(_)
            | Expr::Closure { .. }
            | Expr::New { .. }
            | Expr::NewDynamic { .. }
            | Expr::NewDynamicSpread { .. }
    )
}

pub(crate) fn small_body(params: &[Param], body: &[Stmt]) -> bool {
    let mut scan = Scan::default();
    for p in params {
        if let Some(default) = &p.default {
            scan.expr(default);
        }
    }
    scan.stmts(body);
    scan.nodes <= NODE_BUDGET
}
