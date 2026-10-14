//! Array creation families, derived from value flow rather than producer names.
//! Only keys whose writes reach a creation point reserve slots there. Other
//! writes use the ordinary holder's overflow/unknown-receiver machinery.
use perry_hir::{Expr, Module, Stmt};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
enum Node {
    Expr(usize),
    Local(usize, u32),
    Global(u32),
    Return(usize),
    Field(String),
}
fn expr(e: &Expr) -> Node {
    Node::Expr(e as *const Expr as usize)
}

#[derive(Default)]
struct Flow {
    incoming: HashMap<Node, Vec<Node>>,
    births: Vec<usize>,
    writes: Vec<(Node, String)>,
    refs: Vec<(Node, u32)>,
    calls: Vec<(Node, Node, Vec<Node>)>,
    params: HashMap<u32, Vec<Node>>,
}
impl Flow {
    fn edge(&mut self, source: Node, target: Node) {
        self.incoming.entry(target).or_default().push(source);
    }
    fn expression(&mut self, scope: usize, e: &Expr) {
        let here = expr(e);
        match e {
            Expr::Array(_) | Expr::ArraySlice { .. } | Expr::ArrayFrom(_) => {
                self.births.push(e as *const Expr as usize);
            }
            Expr::LocalGet(id) => self.edge(Node::Local(scope, *id), here.clone()),
            Expr::GlobalGet(id) => self.edge(Node::Global(*id), here.clone()),
            Expr::LocalSet(id, value) => {
                self.edge(expr(value), Node::Local(scope, *id));
                self.edge(expr(value), here.clone());
            }
            Expr::GlobalSet(id, value) => {
                self.edge(expr(value), Node::Global(*id));
                self.edge(expr(value), here.clone());
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
                self.edge(expr(then_expr), here.clone());
                self.edge(expr(else_expr), here.clone());
            }
            Expr::Sequence(items) => {
                if let Some(last) = items.last() {
                    self.edge(expr(last), here.clone());
                }
            }
            Expr::ScopedTemp { id, value, body } => {
                self.edge(expr(value), Node::Local(scope, *id));
                self.edge(expr(body), here.clone());
            }
            Expr::PropertyGet { property, .. } => {
                self.edge(Node::Field(property.clone()), here.clone());
            }
            Expr::PropertySet {
                object,
                property,
                value,
            } => {
                self.write(expr(object), property);
                self.edge(expr(value), Node::Field(property.clone()));
                self.edge(expr(value), here.clone());
            }
            Expr::IndexSet {
                object,
                index,
                value,
            } => {
                if let Expr::String(key) = index.as_ref() {
                    self.write(expr(object), key);
                    self.edge(expr(value), Node::Field(key.clone()));
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
                    self.edge(expr(value), Node::Field(key.clone()));
                }
                self.edge(expr(value), here.clone());
            }
            Expr::IndexGet { index, .. } => {
                if let Expr::String(key) = index.as_ref() {
                    self.edge(Node::Field(key.clone()), here.clone());
                }
            }
            Expr::ReflectGet { key, .. } => {
                if let Expr::String(key) = key.as_ref() {
                    self.edge(Node::Field(key.clone()), here.clone());
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
                    self.edge(expr(value), Node::Field(key.clone()));
                }
            }
            Expr::FuncRef(id) => self.refs.push((here.clone(), *id)),
            Expr::Call { callee, args, .. } => {
                self.calls
                    .push((here.clone(), expr(callee), args.iter().map(expr).collect()));
            }
            Expr::Closure {
                body,
                params,
                captures,
                func_id,
                ..
            } => {
                let nested = *func_id as usize;
                self.refs.push((here.clone(), *func_id));
                self.params.insert(
                    *func_id,
                    params.iter().map(|p| Node::Local(nested, p.id)).collect(),
                );
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
    fn write(&mut self, receiver: Node, key: &str) {
        if key != "length"
            && key != "__proto__"
            && !key.is_empty()
            && key.len() <= 64
            && key.parse::<u32>().is_err()
        {
            self.writes.push((receiver, key.to_owned()));
        }
    }
    fn statements(&mut self, scope: usize, body: &[Stmt]) {
        for stmt in body {
            match stmt {
                Stmt::Let {
                    id, init: Some(e), ..
                } => {
                    self.edge(expr(e), Node::Local(scope, *id));
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
        let mut outgoing: HashMap<Node, Vec<Node>> = HashMap::new();
        for (to, sources) in &self.incoming {
            for from in sources {
                outgoing.entry(from.clone()).or_default().push(to.clone());
            }
        }
        let mut targets: HashMap<Node, BTreeSet<u32>> = HashMap::new();
        let mut work: VecDeque<(Node, u32)> = self.refs.iter().cloned().collect();
        while let Some((node, id)) = work.pop_front() {
            if !targets.entry(node.clone()).or_default().insert(id) {
                continue;
            }
            if let Some(next) = outgoing.get(&node) {
                work.extend(next.iter().cloned().map(|n| (n, id)));
            }
        }
        for (call, callee, args) in self.calls.clone() {
            if let Some(ids) = targets.get(&callee) {
                for id in ids {
                    self.edge(Node::Return(*id as usize), call.clone());
                    if let Some(params) = self.params.get(id).cloned() {
                        for (arg, param) in args.iter().zip(params) {
                            self.edge(arg.clone(), param);
                        }
                    }
                }
            }
        }
    }
}

thread_local! {
    static BIRTH_LAYOUTS: RefCell<HashMap<usize, String>> = RefCell::new(HashMap::new());
}
pub(crate) fn birth_layout(e: &Expr) -> Option<String> {
    BIRTH_LAYOUTS.with(|m| m.borrow().get(&(e as *const Expr as usize)).cloned())
}

pub(crate) fn collect(hir: &Module, prefix: &str) -> Vec<super::FamilyLayout> {
    let mut flow = Flow::default();
    flow.statements(usize::MAX, &hir.init);
    for global in &hir.globals {
        if let Some(e) = &global.init {
            flow.edge(expr(e), Node::Global(global.id));
            flow.expression(usize::MAX, e);
        }
    }
    for function in &hir.functions {
        flow.params.insert(
            function.id,
            function
                .params
                .iter()
                .map(|p| Node::Local(function.id as usize, p.id))
                .collect(),
        );
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
    let mut keys: HashMap<Node, BTreeSet<usize>> = HashMap::new();
    let mut work: VecDeque<(Node, usize)> = flow
        .writes
        .iter()
        .enumerate()
        .map(|(order, (node, _))| (node.clone(), order))
        .collect();
    while let Some((node, order)) = work.pop_front() {
        if !keys.entry(node.clone()).or_default().insert(order) {
            continue;
        }
        if let Some(sources) = flow.incoming.get(&node) {
            work.extend(sources.iter().cloned().map(|n| (n, order)));
        }
    }
    let mut layouts = Vec::new();
    let mut map = HashMap::new();
    let mut shared: BTreeMap<Vec<String>, String> = BTreeMap::new();
    for (ordinal, birth) in flow.births.into_iter().enumerate() {
        let Some(writes) = keys.get(&Node::Expr(birth)) else {
            continue;
        };
        let mut names = Vec::new();
        for order in writes {
            let name = &flow.writes[*order].1;
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        if names.is_empty() {
            continue;
        }
        let desc = shared
            .entry(names.clone())
            .or_insert_with(|| {
                let desc = format!("perry_array_layout_{prefix}_{ordinal}");
                layouts.push(super::FamilyLayout {
                    desc_global: desc.clone(),
                    slot_count: names.len() as u32,
                    keys: names.clone(),
                    rep: 0,
                    array_named: true,
                });
                desc
            })
            .clone();
        map.insert(birth, desc);
    }
    BIRTH_LAYOUTS.with(|m| *m.borrow_mut() = map);
    layouts
}

pub(crate) fn clear() {
    BIRTH_LAYOUTS.with(|m| m.borrow_mut().clear());
}
