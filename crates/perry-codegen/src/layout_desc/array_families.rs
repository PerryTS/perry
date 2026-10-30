//! Array creation families, derived from value flow rather than producer names.
//! Only keys whose writes reach a creation point reserve slots there. Other
//! writes use the ordinary holder's overflow/unknown-receiver machinery.
//!
//! Facts move through one graph pass: the flow graph's strongly connected
//! components are visited in topological order, and each component's sorted
//! set of items is merged into its successors once.
//!
//! The result is HIR data: `assign_birth_layouts` puts each birth's layout on
//! the birth expression before codegen, and lowering reads it there.
use perry_hir::fx::FxHashMap;
use perry_hir::{ArrayBirthLayout, Expr, Module, Stmt};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
enum Node<'a> {
    Expr(usize),
    Local(usize, u32),
    Global(u32),
    Return(usize),
    Field(u32, &'a str),
}
fn expr<'a>(e: &Expr) -> Node<'a> {
    Node::Expr(e as *const Expr as usize)
}

/// Value flow between interned nodes: `edges` holds (source, target) pairs.
#[derive(Default)]
struct Flow<'a> {
    ids: FxHashMap<Node<'a>, u32>,
    edges: Vec<(u32, u32)>,
    births: Vec<usize>,
    writes: Vec<(u32, &'a str)>,
    refs: Vec<(u32, u32)>,
    calls: Vec<(u32, u32, Vec<u32>)>,
    params: FxHashMap<u32, Vec<u32>>,
    /// The CJS wrapper's export snapshots, which codegen never initializes
    /// (`cjs_exports::prepare`).
    snapshots: HashSet<u32>,
}
impl<'a> Flow<'a> {
    fn node(&mut self, node: Node<'a>) -> u32 {
        let next = self.ids.len() as u32;
        *self.ids.entry(node).or_insert(next)
    }
    fn edge(&mut self, source: Node<'a>, target: Node<'a>) {
        let (source, target) = (self.node(source), self.node(target));
        self.edges.push((source, target));
    }
    // Property spelling alone is not a holder identity: joining every `x.items`
    // in a module makes unrelated array births reserve the union of their keys.
    // Only follow fields of the same directly identifiable receiver. An alias
    // or a dynamic receiver we cannot connect simply uses ordinary named adds;
    // a birth layout is a reservation, not a completeness assertion.
    fn field(&mut self, scope: usize, object: &Expr, key: &'a str) -> Node<'a> {
        let receiver = match object {
            Expr::LocalGet(id) => Node::Local(scope, *id),
            Expr::GlobalGet(id) => Node::Global(*id),
            _ => expr(object),
        };
        Node::Field(self.node(receiver), key)
    }
    fn expression(&mut self, scope: usize, e: &'a Expr) {
        let here = expr(e);
        match e {
            Expr::Array(_, _) | Expr::ArraySlice { .. } | Expr::ArrayFrom(_, _) => {
                self.births.push(e as *const Expr as usize);
            }
            Expr::LocalGet(id) => self.edge(Node::Local(scope, *id), here),
            Expr::GlobalGet(id) => self.edge(Node::Global(*id), here),
            Expr::LocalSet(id, value) => {
                self.edge(expr(value), Node::Local(scope, *id));
                self.edge(expr(value), here);
            }
            Expr::GlobalSet(id, value) => {
                self.edge(expr(value), Node::Global(*id));
                self.edge(expr(value), here);
            }
            Expr::Conditional {
                then_expr,
                else_expr,
                ..
            }
            | Expr::Logical {
                left: then_expr,
                right: else_expr,
                ..
            } => {
                self.edge(expr(then_expr), here);
                self.edge(expr(else_expr), here);
            }
            Expr::Sequence(items) => {
                if let Some(last) = items.last() {
                    self.edge(expr(last), here);
                }
            }
            Expr::ScopedTemp { id, value, body } => {
                self.edge(expr(value), Node::Local(scope, *id));
                self.edge(expr(body), here);
            }
            Expr::PropertyGet {
                object, property, ..
            } => {
                let field = self.field(scope, object, property);
                self.edge(field, here);
            }
            Expr::PropertySet {
                object,
                property,
                value,
                ..
            } => {
                self.write(expr(object), property);
                let field = self.field(scope, object, property);
                self.edge(expr(value), field);
                self.edge(expr(value), here);
            }
            Expr::IndexSet {
                object,
                index,
                value,
                ..
            } => {
                if let Expr::String(key) = index.as_ref() {
                    self.write(expr(object), key);
                    let field = self.field(scope, object, key);
                    self.edge(expr(value), field);
                }
            }
            Expr::PutValueSet {
                receiver,
                key,
                value,
                ..
            }
            | Expr::ReflectSet {
                receiver,
                key,
                value,
                ..
            } => {
                if let Expr::String(key) = key.as_ref() {
                    self.write(expr(receiver), key);
                    let field = self.field(scope, receiver, key);
                    self.edge(expr(value), field);
                }
                self.edge(expr(value), here);
            }
            Expr::IndexGet { object, index, .. } => {
                if let Expr::String(key) = index.as_ref() {
                    let field = self.field(scope, object, key);
                    self.edge(field, here);
                }
            }
            Expr::ReflectGet { target, key, .. } => {
                if let Expr::String(key) = key.as_ref() {
                    let field = self.field(scope, target, key);
                    self.edge(field, here);
                }
            }
            Expr::PropertyUpdate {
                object, property, ..
            } => self.write(expr(object), property),
            Expr::IndexUpdate { object, index, .. } => {
                if let Expr::String(key) = index.as_ref() {
                    self.write(expr(object), key);
                }
            }
            Expr::Object(props) => {
                for (key, value) in props {
                    let field = self.field(scope, e, key);
                    self.edge(expr(value), field);
                }
            }
            Expr::FuncRef(id) => {
                let here = self.node(here);
                self.refs.push((here, *id));
            }
            Expr::Call { callee, args, .. } => {
                let call = (self.node(here), self.node(expr(callee)));
                let args = args.iter().map(|a| self.node(expr(a))).collect();
                self.calls.push((call.0, call.1, args));
            }
            Expr::Closure {
                body,
                params,
                captures,
                func_id,
                ..
            } => {
                let nested = *func_id as usize;
                let here = self.node(here);
                self.refs.push((here, *func_id));
                let locals = params
                    .iter()
                    .map(|p| self.node(Node::Local(nested, p.id)))
                    .collect();
                self.params.insert(*func_id, locals);
                for id in captures {
                    self.edge(Node::Local(scope, *id), Node::Local(nested, *id));
                }
                for param in params {
                    if let Some(default) = &param.default {
                        self.expression(nested, default);
                        self.edge(expr(default), Node::Local(nested, param.id));
                    }
                }
                self.statements(nested, body);
                return;
            }
            _ => {}
        }
        perry_hir::walker::walk_expr_children(e, &mut |child| self.expression(scope, child));
    }
    fn write(&mut self, receiver: Node<'a>, key: &'a str) {
        if key != "length"
            && key != "__proto__"
            && !key.is_empty()
            && key.len() <= 64
            && key.parse::<u32>().is_err()
        {
            let receiver = self.node(receiver);
            self.writes.push((receiver, key));
        }
    }
    fn statements(&mut self, scope: usize, body: &'a [Stmt]) {
        for stmt in body {
            match stmt {
                Stmt::Let {
                    id, init: Some(e), ..
                } if !self.snapshots.contains(id) => {
                    self.edge(expr(e), Node::Local(scope, *id));
                    if let Expr::Object(props) = e {
                        for (key, value) in props {
                            let field = self.field(scope, &Expr::LocalGet(*id), key);
                            self.edge(expr(value), field);
                        }
                    }
                    self.expression(scope, e);
                }
                Stmt::Return(Some(e)) => {
                    self.edge(expr(e), Node::Return(scope));
                    self.expression(scope, e);
                }
                Stmt::Expr(e) | Stmt::Throw(e) => self.expression(scope, e),
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    self.expression(scope, condition);
                    self.statements(scope, then_branch);
                    if let Some(body) = else_branch {
                        self.statements(scope, body);
                    }
                }
                Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                    self.expression(scope, condition);
                    self.statements(scope, body);
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    if let Some(init) = init {
                        self.statements(scope, std::slice::from_ref(init));
                    }
                    if let Some(e) = condition {
                        self.expression(scope, e);
                    }
                    if let Some(e) = update {
                        self.expression(scope, e);
                    }
                    self.statements(scope, body);
                }
                Stmt::Labeled { body, .. } => self.statements(scope, std::slice::from_ref(body)),
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    self.statements(scope, body);
                    if let Some(c) = catch {
                        self.statements(scope, &c.body);
                    }
                    if let Some(f) = finally {
                        self.statements(scope, f);
                    }
                }
                Stmt::Switch {
                    discriminant,
                    cases,
                } => {
                    self.expression(scope, discriminant);
                    for case in cases {
                        if let Some(e) = &case.test {
                            self.expression(scope, e);
                        }
                        self.statements(scope, &case.body);
                    }
                }
                _ => {}
            }
        }
    }
    fn connect_calls(&mut self) {
        // Resolve direct function values through the same assignment graph.
        // Unknown callees add no proof; their writes remain ordinary accesses.
        let forward = Graph::new(self.ids.len(), self.edges.iter().copied());
        let callees: Vec<u32> = self.calls.iter().map(|(_, callee, _)| *callee).collect();
        let targets = forward.propagate(&self.refs, &callees);
        for (call, callee, args) in std::mem::take(&mut self.calls) {
            for &id in targets.at(callee) {
                let ret = self.node(Node::Return(id as usize));
                self.edges.push((ret, call));
                if let Some(params) = self.params.get(&id) {
                    for (arg, param) in args.iter().zip(params) {
                        self.edges.push((*arg, *param));
                    }
                }
            }
        }
    }
}

/// A directed graph over node ids `0..n`, as adjacency in one array.
struct Graph {
    start: Vec<u32>,
    to: Vec<u32>,
}

/// Item sets at the queried nodes: per strongly connected component, sorted.
struct Reached {
    component: Vec<u32>,
    sets: Vec<Vec<u32>>,
}
impl Reached {
    fn at(&self, node: u32) -> &[u32] {
        self.component
            .get(node as usize)
            .map_or(&[], |c| &self.sets[*c as usize])
    }
}

impl Graph {
    fn new(n: usize, edges: impl Iterator<Item = (u32, u32)> + Clone) -> Self {
        let mut start = vec![0u32; n + 1];
        for (from, _) in edges.clone() {
            start[from as usize + 1] += 1;
        }
        for i in 0..n {
            start[i + 1] += start[i];
        }
        let mut fill = start.clone();
        let mut to = vec![0u32; start[n] as usize];
        for (from, target) in edges {
            to[fill[from as usize] as usize] = target;
            fill[from as usize] += 1;
        }
        Graph { start, to }
    }
    fn successors(&self, node: u32) -> &[u32] {
        &self.to[self.start[node as usize] as usize..self.start[node as usize + 1] as usize]
    }
    fn len(&self) -> usize {
        self.start.len() - 1
    }
    /// Strongly connected components (iterative Tarjan). A component is
    /// numbered after every component it reaches, so for an edge between two
    /// components the target's number is the smaller one.
    fn components(&self) -> (Vec<u32>, usize) {
        const NONE: u32 = u32::MAX;
        let n = self.len();
        let mut index = vec![NONE; n];
        let mut low = vec![0u32; n];
        let mut component = vec![NONE; n];
        let mut stack: Vec<u32> = Vec::new();
        let mut calls: Vec<(u32, u32)> = Vec::new();
        let (mut next, mut count) = (0u32, 0u32);
        for root in 0..n as u32 {
            if index[root as usize] != NONE {
                continue;
            }
            calls.push((root, 0));
            while let Some(&mut (v, ref mut edge)) = calls.last_mut() {
                let vi = v as usize;
                if *edge == 0 && index[vi] == NONE {
                    index[vi] = next;
                    low[vi] = next;
                    next += 1;
                    stack.push(v);
                }
                let succ = self.successors(v);
                if let Some(&w) = succ.get(*edge as usize) {
                    *edge += 1;
                    let wi = w as usize;
                    if index[wi] == NONE {
                        calls.push((w, 0));
                    } else if component[wi] == NONE {
                        low[vi] = low[vi].min(index[wi]);
                    }
                    continue;
                }
                calls.pop();
                if low[vi] == index[vi] {
                    loop {
                        let w = stack.pop().expect("tarjan stack");
                        component[w as usize] = count;
                        if w == v {
                            break;
                        }
                    }
                    count += 1;
                }
                if let Some(&(parent, _)) = calls.last() {
                    let pi = parent as usize;
                    low[pi] = low[pi].min(low[vi]);
                }
            }
        }
        (component, count as usize)
    }
    /// The items seeded at nodes, flowed along every edge: each component's
    /// set is complete once all components that reach it are done, then it
    /// merges into its successors once. Only the queried nodes' sets are kept.
    fn propagate(&self, seeds: &[(u32, u32)], queries: &[u32]) -> Reached {
        let (component, count) = self.components();
        let members = Graph::new(
            count,
            component
                .iter()
                .enumerate()
                .map(|(node, c)| (*c, node as u32)),
        );
        let mut sets: Vec<Vec<u32>> = vec![Vec::new(); count];
        for (node, item) in seeds {
            sets[component[*node as usize] as usize].push(*item);
        }
        for set in &mut sets {
            set.sort_unstable();
            set.dedup();
        }
        let mut keep = vec![false; count];
        for node in queries {
            if let Some(c) = component.get(*node as usize) {
                keep[*c as usize] = true;
            }
        }
        let mut pushed = vec![u32::MAX; count];
        for c in (0..count).rev() {
            let set = std::mem::take(&mut sets[c]);
            if !set.is_empty() {
                for &v in members.successors(c as u32) {
                    for &w in self.successors(v) {
                        let d = component[w as usize] as usize;
                        if d != c && pushed[d] != c as u32 {
                            pushed[d] = c as u32;
                            merge_into(&mut sets[d], &set);
                        }
                    }
                }
            }
            if keep[c] {
                sets[c] = set;
            }
        }
        Reached { component, sets }
    }
}

/// `into ∪= from`, both sorted and free of duplicates.
fn merge_into(into: &mut Vec<u32>, from: &[u32]) {
    if into.is_empty() {
        into.extend_from_slice(from);
        return;
    }
    let mut out = Vec::with_capacity(into.len() + from.len());
    let (mut i, mut j) = (0, 0);
    while i < into.len() && j < from.len() {
        let (a, b) = (into[i], from[j]);
        out.push(a.min(b));
        i += (a <= b) as usize;
        j += (b <= a) as usize;
    }
    out.extend_from_slice(&into[i..]);
    out.extend_from_slice(&from[j..]);
    *into = out;
}

/// The layout descriptor `e` is born with: an array birth whose named
/// properties have a layout, in a module that emits array layouts.
pub(crate) fn birth_layout(e: &Expr) -> Option<String> {
    let layout = match e {
        Expr::Array(_, layout) | Expr::ArraySlice { layout, .. } | Expr::ArrayFrom(_, layout) => {
            layout.as_ref()?
        }
        _ => return None,
    };
    super::array_layout_desc(layout.ordinal)
}

/// Put each array birth's layout on the birth: the keys whose writes reach
/// it, in the order of their first write. Runs once per module before
/// codegen; clones of the HIR carry the layout with them.
pub fn assign_birth_layouts(hir: &mut Module) {
    let layouts = birth_layouts(hir);
    if layouts.is_empty() {
        return;
    }
    let mut set = |e: &mut Expr| {
        let at = e as *const Expr as usize;
        if let Expr::Array(_, layout)
        | Expr::ArraySlice { layout, .. }
        | Expr::ArrayFrom(_, layout) = e
        {
            if let Some(found) = layouts.get(&at) {
                *layout = Some(found.clone());
            }
        }
    };
    for_each_birth_mut(hir, &mut set);
}

/// The layout of every birth that has one, by the birth's address.
fn birth_layouts(hir: &Module) -> FxHashMap<usize, Arc<ArrayBirthLayout>> {
    let mut flow = Flow {
        snapshots: crate::codegen::cjs_exports::snapshot_bindings(hir),
        ..Flow::default()
    };
    flow.statements(usize::MAX, &hir.init);
    for global in &hir.globals {
        if let Some(e) = &global.init {
            flow.edge(expr(e), Node::Global(global.id));
            flow.expression(usize::MAX, e);
        }
    }
    for function in &hir.functions {
        let locals = function
            .params
            .iter()
            .map(|p| flow.node(Node::Local(function.id as usize, p.id)))
            .collect();
        flow.params.insert(function.id, locals);
        flow.statements(function.id as usize, &function.body);
    }
    // Class method scopes are distinct from top-level functions.
    for class in &hir.classes {
        for function in class
            .constructor
            .iter()
            .chain(&class.methods)
            .chain(&class.static_methods)
            .chain(class.getters.iter().map(|(_, f)| f))
            .chain(class.setters.iter().map(|(_, f)| f))
        {
            flow.statements(function as *const _ as usize, &function.body);
        }
    }
    flow.connect_calls();
    // A write's key reaches every value that flows into its receiver: walk
    // the edges backwards, from target to source.
    let backward = Graph::new(
        flow.ids.len(),
        flow.edges.iter().map(|&(source, target)| (target, source)),
    );
    let seeds: Vec<(u32, u32)> = flow
        .writes
        .iter()
        .enumerate()
        .map(|(order, (node, _))| (*node, order as u32))
        .collect();
    let birth_nodes: Vec<Option<u32>> = flow
        .births
        .iter()
        .map(|b| flow.ids.get(&Node::Expr(*b)).copied())
        .collect();
    let queries: Vec<u32> = birth_nodes.iter().flatten().copied().collect();
    let keys = backward.propagate(&seeds, &queries);
    let mut out = FxHashMap::default();
    let mut shared: BTreeMap<Vec<String>, Arc<ArrayBirthLayout>> = BTreeMap::new();
    for (ordinal, (birth, node)) in flow.births.iter().zip(&birth_nodes).enumerate() {
        let Some(node) = node else {
            continue;
        };
        // Keys in the order of their first write.
        let mut names: Vec<String> = Vec::new();
        for order in keys.at(*node) {
            let name = flow.writes[*order as usize].1;
            if !names.iter().any(|n| n == name) {
                names.push(name.to_owned());
            }
        }
        if names.is_empty() {
            continue;
        }
        let layout = shared
            .entry(names.clone())
            .or_insert_with(|| {
                Arc::new(ArrayBirthLayout {
                    ordinal: ordinal as u32,
                    keys: names,
                })
            })
            .clone();
        out.insert(*birth, layout);
    }
    out
}

/// Visit every array birth of the module (`birth_layouts` reads them in
/// function bodies, class members, globals and the module init).
fn for_each_birth_mut(hir: &mut Module, f: &mut impl FnMut(&mut Expr)) {
    stmts_mut(&mut hir.init, f);
    for global in &mut hir.globals {
        if let Some(e) = &mut global.init {
            expr_mut(e, f);
        }
    }
    for function in &mut hir.functions {
        stmts_mut(&mut function.body, f);
    }
    for class in &mut hir.classes {
        for function in class
            .constructor
            .iter_mut()
            .chain(&mut class.methods)
            .chain(&mut class.static_methods)
            .chain(class.getters.iter_mut().map(|(_, f)| f))
            .chain(class.setters.iter_mut().map(|(_, f)| f))
        {
            stmts_mut(&mut function.body, f);
        }
    }
}

fn expr_mut(e: &mut Expr, f: &mut impl FnMut(&mut Expr)) {
    f(e);
    if let Expr::Closure { body, .. } = e {
        stmts_mut(body, f);
    }
    perry_hir::walker::walk_expr_children_mut(e, &mut |child| expr_mut(child, f));
}

fn stmts_mut(body: &mut [Stmt], f: &mut impl FnMut(&mut Expr)) {
    for stmt in body {
        match stmt {
            Stmt::Let { init: Some(e), .. }
            | Stmt::Expr(e)
            | Stmt::Throw(e)
            | Stmt::Return(Some(e)) => expr_mut(e, f),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                expr_mut(condition, f);
                stmts_mut(then_branch, f);
                if let Some(body) = else_branch {
                    stmts_mut(body, f);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                expr_mut(condition, f);
                stmts_mut(body, f);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    stmts_mut(std::slice::from_mut(init), f);
                }
                if let Some(e) = condition {
                    expr_mut(e, f);
                }
                if let Some(e) = update {
                    expr_mut(e, f);
                }
                stmts_mut(body, f);
            }
            Stmt::Labeled { body, .. } => stmts_mut(std::slice::from_mut(body), f),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                stmts_mut(body, f);
                if let Some(c) = catch {
                    stmts_mut(&mut c.body, f);
                }
                if let Some(body) = finally {
                    stmts_mut(body, f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                expr_mut(discriminant, f);
                for case in cases {
                    if let Some(e) = &mut case.test {
                        expr_mut(e, f);
                    }
                    stmts_mut(&mut case.body, f);
                }
            }
            _ => {}
        }
    }
}

/// This module's array layouts, from the layouts its births carry: one per
/// distinct layout, in birth order.
pub(crate) fn collect(hir: &Module, prefix: &str) -> Vec<super::FamilyLayout> {
    let mut layouts: BTreeMap<u32, Arc<ArrayBirthLayout>> = BTreeMap::new();
    perry_hir::for_each_module_expr(hir, &mut |e| {
        if let Expr::Array(_, Some(layout))
        | Expr::ArraySlice {
            layout: Some(layout),
            ..
        }
        | Expr::ArrayFrom(_, Some(layout)) = e
        {
            layouts
                .entry(layout.ordinal)
                .or_insert_with(|| layout.clone());
        }
    });
    layouts
        .into_values()
        .map(|layout| super::FamilyLayout {
            desc_global: super::array_layout_symbol(prefix, layout.ordinal),
            slot_count: layout.keys.len() as u32,
            keys: layout.keys.clone(),
            rep: 0,
            array_named: true,
        })
        .collect()
}

#[cfg(test)]
#[path = "array_families_tests.rs"]
mod tests;
