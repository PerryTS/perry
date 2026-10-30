//! The flow-insensitive escape graph behind `PERRY_SOURCE_NEED_REPORT`.
//!
//! Nodes carry function/class values: the value itself, locals, module
//! globals, return slots and export bindings. An edge `a -> b` means every
//! value `a` may hold, `b` may hold too. Sink nodes are the places a value can
//! leave what this graph models: a string conversion, an object property (the
//! heap is object-insensitive here, so any property store is an escape), an
//! unresolved callee, a builtin, an export, dynamic code. A function or class
//! whose value node reaches no sink can never be stringified.
//!
//! Soundness rule for everything this file does not model: every operand of an
//! unlisted HIR expression that may hold a function value flows to a sink named
//! after that expression. Reads out of the heap return no tracked value, which
//! is sound because every value stored into the heap already reached a sink.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use perry_hir::fx::{FxHashMap, FxHashSet};
use perry_hir::{ArrayElement, CallArg, Class, Expr, Function, Module, Stmt};

/// Why a value may be stringified. Grouped by [`Reason::category`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Reason {
    // Known string sinks.
    ToPrimitive,
    ToStringBuiltin,
    ToStringCall,
    FnProtoToStringCall,
    ToPropertyKey,
    ErrorMessage,
    // Escapes no static analysis here can follow (dynamic code, exports...).
    Export,
    LibraryExport,
    NamespaceObject,
    EvalScope,
    DirectEvalModule,
    DynCodeGlobals,
    GlobalPublication,
    With,
    Worker,
    Reflection,
    ArgumentsCallee,
    // Unknown sinks: an untracked callee, the heap, a builtin.
    HeapStore,
    ClassMember,
    Instantiated,
    PrototypeRead,
    StaticThis,
    IndirectCallArg,
    MethodReceiver,
    ArgumentsObject,
    RestArgs,
    ReturnFromValueFn,
    AsyncReturn,
    Yield,
    Throw,
    Decorator,
    InstanceofHook,
    Builtin,
}

pub const REASONS: [Reason; 33] = [
    Reason::ToPrimitive,
    Reason::ToStringBuiltin,
    Reason::ToStringCall,
    Reason::FnProtoToStringCall,
    Reason::ToPropertyKey,
    Reason::ErrorMessage,
    Reason::Export,
    Reason::LibraryExport,
    Reason::NamespaceObject,
    Reason::EvalScope,
    Reason::DirectEvalModule,
    Reason::DynCodeGlobals,
    Reason::GlobalPublication,
    Reason::With,
    Reason::Worker,
    Reason::Reflection,
    Reason::ArgumentsCallee,
    Reason::HeapStore,
    Reason::ClassMember,
    Reason::Instantiated,
    Reason::PrototypeRead,
    Reason::StaticThis,
    Reason::IndirectCallArg,
    Reason::MethodReceiver,
    Reason::ArgumentsObject,
    Reason::RestArgs,
    Reason::ReturnFromValueFn,
    Reason::AsyncReturn,
    Reason::Yield,
    Reason::Throw,
    Reason::Decorator,
    Reason::InstanceofHook,
    Reason::Builtin,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Reaches a definite string conversion.
    Known,
    /// Escapes into dynamic code, an export or reflection.
    Hard,
    /// Escapes only into an untracked callee, the heap or a builtin.
    Unknown,
}

impl Reason {
    pub fn bit(self) -> u64 {
        1u64 << (self as u32)
    }
    pub fn category(self) -> Category {
        use Reason::*;
        match self {
            ToPrimitive | ToStringBuiltin | ToStringCall | FnProtoToStringCall | ToPropertyKey
            | ErrorMessage => Category::Known,
            Export | LibraryExport | EvalScope | DirectEvalModule | DynCodeGlobals
            | GlobalPublication | With | Worker | Reflection | ArgumentsCallee => Category::Hard,
            _ => Category::Unknown,
        }
    }
    pub fn name(self) -> &'static str {
        use Reason::*;
        match self {
            ToPrimitive => "to-primitive-operator",
            ToStringBuiltin => "to-string-builtin",
            ToStringCall => "toString-call",
            FnProtoToStringCall => "Function.prototype.toString.call",
            ToPropertyKey => "to-property-key",
            ErrorMessage => "class-source-in-property-write-error",
            Export => "export",
            LibraryExport => "library-export",
            NamespaceObject => "namespace-object",
            EvalScope => "eval-scope",
            DirectEvalModule => "direct-eval-module",
            DynCodeGlobals => "dynamic-code-globals",
            GlobalPublication => "global-publication",
            With => "with",
            Worker => "worker",
            Reflection => "proxy-reflect",
            ArgumentsCallee => "arguments.callee",
            HeapStore => "heap-store",
            ClassMember => "class-member",
            Instantiated => "instantiated",
            PrototypeRead => "prototype-read",
            StaticThis => "static-this",
            IndirectCallArg => "indirect-call-arg",
            MethodReceiver => "method-receiver",
            ArgumentsObject => "arguments-object",
            RestArgs => "rest-args",
            ReturnFromValueFn => "return-of-escaped-function",
            AsyncReturn => "async-generator-return",
            Yield => "yield",
            Throw => "throw",
            Decorator => "decorator",
            InstanceofHook => "instanceof-hasInstance",
            Builtin => "builtin",
        }
    }
}

pub fn category_mask(category: Category) -> u64 {
    REASONS
        .iter()
        .filter(|r| r.category() == category)
        .fold(0, |m, r| m | r.bit())
}

/// A source owner: a function body (by module index and FuncId) or a class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ent {
    Func(usize, u32),
    Class(usize, u32),
}

#[derive(Debug, Clone)]
pub struct EntityInfo {
    pub ent: Ent,
    pub label: String,
    pub kind: &'static str,
}

/// The census result. `mask[e]` holds every reason bit the entity's value can
/// reach; `witness[e]` names the highest-priority sink it reaches.
#[derive(Debug)]
pub struct Need {
    pub entities: Vec<EntityInfo>,
    pub index: FxHashMap<Ent, usize>,
    pub mask: Vec<u64>,
    pub valued: Vec<bool>,
    pub witness: Vec<Option<(Reason, String)>>,
    /// HIR module names, by module index.
    pub module_names: Vec<String>,
    pub module_paths: Vec<String>,
    pub nodes: usize,
    pub edges: usize,
    /// Diagnostic counters (e.g. why a namespace was materialized).
    pub notes: BTreeMap<String, usize>,
    pub samples: Vec<String>,
}

/// Classification of one entity under a configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class3 {
    /// (a) never a value.
    NeverValue,
    /// (b) a value, but no sink is reachable.
    NoSink,
    /// (c) may be stringified.
    MayStringify,
}

impl Class3 {
    pub fn letter(self) -> &'static str {
        match self {
            Class3::NeverValue => "a",
            Class3::NoSink => "b",
            Class3::MayStringify => "c",
        }
    }
}

impl Need {
    /// Classify with `ignored` reason bits treated as harmless.
    pub fn class_of(&self, e: usize, ignored: u64) -> Class3 {
        if self.mask[e] & !ignored != 0 {
            Class3::MayStringify
        } else if self.valued[e] {
            Class3::NoSink
        } else {
            Class3::NeverValue
        }
    }
    pub fn entity(&self, ent: Ent) -> Option<usize> {
        self.index.get(&ent).copied()
    }
    pub fn reasons(&self, e: usize) -> Vec<&'static str> {
        REASONS
            .iter()
            .filter(|r| self.mask[e] & r.bit() != 0)
            .map(|r| r.name())
            .collect()
    }
}

type Vals = Vec<u32>;

#[derive(Clone, Copy)]
struct Cx {
    mi: usize,
    func: Option<u32>,
    /// Returns go to this sink instead of the return slot (async, generator,
    /// constructor and accessor bodies: their callers are never direct).
    ret_sink: Option<Reason>,
    static_this: bool,
}

#[derive(Clone)]
struct Sig {
    params: Vec<(u32, bool)>,
    arguments_object: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Key {
    Ent(Ent),
    Local(usize, u32),
    Global(usize, u32),
    Ret(usize, u32),
    Export(usize, u32),
    StaticThis,
    Sink(u32),
}

enum Resolved {
    Node(u32),
    Namespace(usize),
}

struct Build<'a> {
    modules: &'a [(String, &'a Module)],
    /// Per module: import/re-export specifier -> the linked module's index.
    links: Vec<FxHashMap<&'a str, usize>>,
    paths: FxHashMap<&'a str, usize>,
    keys: FxHashMap<Key, u32>,
    edges: Vec<Vec<u32>>,
    sinks: Vec<(Reason, String)>,
    /// Per reason (`Reason as usize`): detail -> sink id.
    sink_ids: Vec<FxHashMap<String, u32>>,
    export_names: Vec<(usize, String)>,
    /// Per module: export name -> export id.
    export_ids: Vec<FxHashMap<String, u32>>,
    /// Per module: its export ids, in creation order.
    module_exports: Vec<Vec<u32>>,
    entities: Vec<EntityInfo>,
    ent_index: FxHashMap<Ent, usize>,
    valued: FxHashSet<usize>,
    sigs: FxHashMap<(usize, u32), Sig>,
    static_sigs: FxHashMap<(usize, String, String), u32>,
    fn_names: Vec<FxHashMap<&'a str, u32>>,
    class_names: Vec<FxHashMap<&'a str, u32>>,
    global_names: Vec<FxHashMap<&'a str, u32>>,
    global_ids: Vec<FxHashSet<u32>>,
    init_locals: Vec<FxHashMap<&'a str, u32>>,
    namespaces_materialized: BTreeSet<usize>,
    user_call_props: bool,
    has_instance_hook: bool,
    edge_count: usize,
    notes: BTreeMap<String, usize>,
    samples: Vec<String>,
    unresolved_export_modules: BTreeSet<usize>,
    relabel: FxHashMap<Ent, &'static str>,
}

const CALL_PROPS: [&str; 3] = ["call", "apply", "bind"];

impl<'a> Build<'a> {
    fn node(&mut self, key: Key) -> u32 {
        if let Some(&n) = self.keys.get(&key) {
            return n;
        }
        let n = self.edges.len() as u32;
        self.edges.push(Vec::new());
        self.keys.insert(key, n);
        n
    }

    fn sink(&mut self, reason: Reason, detail: &str) -> u32 {
        if let Some(&id) = self.sink_ids[reason as usize].get(detail) {
            return self.node(Key::Sink(id));
        }
        let id = self.sinks.len() as u32;
        self.sinks.push((reason, detail.to_string()));
        self.sink_ids[reason as usize].insert(detail.to_string(), id);
        self.node(Key::Sink(id))
    }

    fn edge(&mut self, from: u32, to: u32) {
        if from != to {
            self.edges[from as usize].push(to);
            self.edge_count += 1;
        }
    }

    fn flow(&mut self, vals: &[u32], to: u32) {
        for &v in vals {
            self.edge(v, to);
        }
    }

    fn sink_vals(&mut self, vals: &[u32], reason: Reason, detail: &str) {
        if vals.is_empty() {
            return;
        }
        let s = self.sink(reason, detail);
        self.flow(vals, s);
    }

    fn entity(&mut self, ent: Ent, label: String, kind: &'static str) -> u32 {
        if !self.ent_index.contains_key(&ent) {
            self.ent_index.insert(ent, self.entities.len());
            self.entities.push(EntityInfo { ent, label, kind });
        }
        self.node(Key::Ent(ent))
    }

    fn mark_valued(&mut self, ent: Ent) {
        if let Some(&e) = self.ent_index.get(&ent) {
            self.valued.insert(e);
        }
    }

    fn export_node(&mut self, mi: usize, name: &str) -> u32 {
        let id = if let Some(&id) = self.export_ids[mi].get(name) {
            id
        } else {
            let id = self.export_names.len() as u32;
            self.export_names.push((mi, name.to_string()));
            self.export_ids[mi].insert(name.to_string(), id);
            self.module_exports[mi].push(id);
            id
        };
        self.node(Key::Export(mi, id))
    }

    fn target(&self, mi: usize, source: &str) -> Option<usize> {
        self.links[mi]
            .get(source)
            .or_else(|| self.paths.get(source))
            .copied()
    }

    fn local(&mut self, mi: usize, id: u32) -> u32 {
        self.node(Key::Local(mi, id))
    }

    fn class_by_name(&self, mi: usize, name: &str) -> Option<u32> {
        self.class_names[mi].get(name).copied()
    }

    /// Resolve a source-level binding name to the node holding its value.
    fn resolve_name(&mut self, mi: usize, name: &str) -> Option<Resolved> {
        if let Some(&fid) = self.fn_names[mi].get(name) {
            self.mark_valued(Ent::Func(mi, fid));
            return Some(Resolved::Node(self.node(Key::Ent(Ent::Func(mi, fid)))));
        }
        if let Some(cid) = self.class_by_name(mi, name) {
            self.mark_valued(Ent::Class(mi, cid));
            return Some(Resolved::Node(self.node(Key::Ent(Ent::Class(mi, cid)))));
        }
        if let Some(&gid) = self.global_names[mi].get(name) {
            return Some(Resolved::Node(self.node(Key::Global(mi, gid))));
        }
        if let Some(&lid) = self.init_locals[mi].get(name) {
            return Some(Resolved::Node(self.local(mi, lid)));
        }
        let m = self.modules[mi].1;
        for import in &m.imports {
            if import.type_only || import.runtime_erased {
                continue;
            }
            let Some(target) = self.target(mi, &import.source) else {
                continue;
            };
            for spec in &import.specifiers {
                let (local, exported) = match spec {
                    perry_hir::ImportSpecifier::Named { imported, local } => {
                        (local.as_str(), Some(imported.as_str()))
                    }
                    perry_hir::ImportSpecifier::Default { local } => {
                        (local.as_str(), Some("default"))
                    }
                    perry_hir::ImportSpecifier::Namespace { local } => (local.as_str(), None),
                };
                if name == local {
                    return Some(match exported {
                        Some(e) => Resolved::Node(self.export_node(target, e)),
                        None => Resolved::Namespace(target),
                    });
                }
                if exported.is_none() {
                    if let Some(member) = name.strip_prefix(local).and_then(|s| s.strip_prefix('.'))
                    {
                        return Some(Resolved::Node(self.export_node(target, member)));
                    }
                }
            }
        }
        None
    }

    /// The function an exported binding finally names, through re-exports.
    fn export_function(&self, mi: usize, name: &str, depth: usize) -> Option<(usize, u32)> {
        if depth > 32 {
            return None;
        }
        let m = self.modules[mi].1;
        if let Some((_, fid)) = m.exported_functions.iter().find(|(n, _)| n == name) {
            return Some((mi, *fid));
        }
        for e in &m.exports {
            match e {
                perry_hir::Export::Named { local, exported } if exported == name => {
                    if let Some(&fid) = self.fn_names[mi].get(local.as_str()) {
                        return Some((mi, fid));
                    }
                    return self.import_function(mi, local, depth + 1);
                }
                perry_hir::Export::ReExport {
                    source,
                    imported,
                    exported,
                } if exported == name => {
                    return self
                        .target(mi, source)
                        .and_then(|t| self.export_function(t, imported, depth + 1));
                }
                _ => {}
            }
        }
        for e in &m.exports {
            if let perry_hir::Export::ExportAll { source } = e {
                if name != "default" {
                    if let Some(found) = self
                        .target(mi, source)
                        .and_then(|t| self.export_function(t, name, depth + 1))
                    {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    /// The module function an imported (or local) callee name designates.
    fn import_function(&self, mi: usize, name: &str, depth: usize) -> Option<(usize, u32)> {
        if let Some(&fid) = self.fn_names[mi].get(name) {
            return Some((mi, fid));
        }
        let m = self.modules[mi].1;
        for import in &m.imports {
            if import.type_only || import.runtime_erased {
                continue;
            }
            let Some(target) = self.target(mi, &import.source) else {
                continue;
            };
            for spec in &import.specifiers {
                match spec {
                    perry_hir::ImportSpecifier::Named { imported, local } if local == name => {
                        return self.export_function(target, imported, depth + 1);
                    }
                    perry_hir::ImportSpecifier::Default { local } if local == name => {
                        return self.export_function(target, "default", depth + 1);
                    }
                    perry_hir::ImportSpecifier::Namespace { local } => {
                        if let Some(member) = name
                            .strip_prefix(local.as_str())
                            .and_then(|s| s.strip_prefix('.'))
                        {
                            return self.export_function(target, member, depth + 1);
                        }
                    }
                    _ => {}
                }
            }
        }
        None
    }

    fn materialize_namespace(&mut self, target: usize) {
        self.namespaces_materialized.insert(target);
    }

    fn note(&mut self, what: &str, sample: impl FnOnce() -> String) {
        let n = self.notes.entry(what.to_string()).or_default();
        *n += 1;
        if *n <= 10 {
            let s = sample();
            self.samples.push(format!("{what}: {s}"));
        }
    }

    // ---- statements ----

    fn stmts(&mut self, cx: Cx, stmts: &'a [Stmt]) {
        for s in stmts {
            self.stmt(cx, s);
        }
    }

    fn stmt(&mut self, cx: Cx, s: &'a Stmt) {
        match s {
            Stmt::Let { id, name, init, .. } => {
                if let Some(e) = init {
                    // The CommonJS wrapper (cjs_wrap): the factory's text is
                    // the whole module; label it for the report.
                    if name == "__perry_cjs_factory" {
                        if let Expr::Closure { func_id, .. } = e {
                            self.relabel
                                .insert(Ent::Func(cx.mi, *func_id), "cjs-factory");
                        }
                    }
                    if name == "_cjs" {
                        if let Expr::Call { callee, .. } = e {
                            if let Expr::Closure { func_id, .. } = callee.as_ref() {
                                self.relabel
                                    .insert(Ent::Func(cx.mi, *func_id), "cjs-wrapper");
                            }
                        }
                    }
                    let v = self.expr(cx, e);
                    let l = self.local(cx.mi, *id);
                    self.flow(&v, l);
                }
            }
            Stmt::Expr(e) => {
                self.expr(cx, e);
            }
            Stmt::Return(e) => {
                if let Some(e) = e {
                    let v = self.expr(cx, e);
                    if let Some(r) = cx.ret_sink {
                        self.sink_vals(&v, r, "return");
                    } else if let Some(f) = cx.func {
                        let r = self.node(Key::Ret(cx.mi, f));
                        self.flow(&v, r);
                    } else {
                        self.sink_vals(&v, Reason::ReturnFromValueFn, "module-return");
                    }
                }
            }
            Stmt::Throw(e) => {
                let v = self.expr(cx, e);
                self.sink_vals(&v, Reason::Throw, "throw");
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(cx, condition);
                self.stmts(cx, then_branch);
                if let Some(b) = else_branch {
                    self.stmts(cx, b);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
                self.expr(cx, condition);
                self.stmts(cx, body);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.stmt(cx, i);
                }
                if let Some(c) = condition {
                    self.expr(cx, c);
                }
                if let Some(u) = update {
                    self.expr(cx, u);
                }
                self.stmts(cx, body);
            }
            Stmt::Labeled { body, .. } => self.stmt(cx, body),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.stmts(cx, body);
                if let Some(c) = catch {
                    self.stmts(cx, &c.body);
                }
                if let Some(f) = finally {
                    self.stmts(cx, f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(cx, discriminant);
                for c in cases {
                    if let Some(t) = &c.test {
                        self.expr(cx, t);
                    }
                    self.stmts(cx, &c.body);
                }
            }
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
        }
    }

    // ---- functions ----

    fn body(
        &mut self,
        mi: usize,
        fid: u32,
        params: &'a [perry_hir::Param],
        body: &'a [Stmt],
        ret_sink: Option<Reason>,
        static_this: bool,
    ) {
        let cx = Cx {
            mi,
            func: Some(fid),
            ret_sink,
            static_this,
        };
        for p in params {
            if let Some(d) = &p.default {
                let v = self.expr(cx, d);
                let l = self.local(mi, p.id);
                self.flow(&v, l);
            }
            if p.arguments_object
                .as_ref()
                .is_some_and(|a| !a.strict && !a.restricted_callee)
            {
                // Sloppy `arguments.callee` hands the function itself to the
                // arguments object.
                let n = self.node(Key::Ent(Ent::Func(mi, fid)));
                self.sink_vals(&[n], Reason::ArgumentsCallee, "arguments.callee");
            }
        }
        self.stmts(cx, body);
    }

    fn function(
        &mut self,
        mi: usize,
        f: &'a Function,
        ret_sink: Option<Reason>,
        static_this: bool,
    ) {
        let ret = if f.is_async || f.is_generator || f.was_plain_async {
            Some(Reason::AsyncReturn)
        } else {
            ret_sink
        };
        for d in &f.decorators {
            let n = self.node(Key::Ent(Ent::Func(mi, f.id)));
            self.sink_vals(&[n], Reason::Decorator, &d.name);
            self.call_args_unknown(
                Cx {
                    mi,
                    func: None,
                    ret_sink: None,
                    static_this: false,
                },
                &d.args,
            );
        }
        self.body(mi, f.id, &f.params, &f.body, ret, static_this);
    }

    fn sig_of(params: &[perry_hir::Param]) -> Sig {
        Sig {
            params: params
                .iter()
                .filter(|p| p.arguments_object.is_none())
                .map(|p| (p.id, p.is_rest))
                .collect(),
            arguments_object: params.iter().any(|p| p.arguments_object.is_some()),
        }
    }

    fn direct_args(&mut self, cx: Cx, tm: usize, sig: &Sig, args: &'a [Expr]) {
        for (i, a) in args.iter().enumerate() {
            let v = self.expr(cx, a);
            if v.is_empty() {
                continue;
            }
            if sig.arguments_object {
                self.sink_vals(&v, Reason::ArgumentsObject, "arguments");
            }
            match sig.params.get(i) {
                Some(&(_, true)) => self.sink_vals(&v, Reason::RestArgs, "rest"),
                Some(&(pid, false)) => {
                    let l = self.local(tm, pid);
                    self.flow(&v, l);
                }
                None => {
                    if let Some(&(_, true)) = sig.params.last() {
                        self.sink_vals(&v, Reason::RestArgs, "rest");
                    }
                }
            }
        }
    }

    fn call_args_unknown(&mut self, cx: Cx, args: &'a [Expr]) {
        for a in args {
            let v = self.expr(cx, a);
            self.sink_vals(&v, Reason::IndirectCallArg, "call");
        }
    }

    fn call(&mut self, cx: Cx, callee: &'a Expr, args: &'a [Expr]) -> Vals {
        match callee {
            Expr::FuncRef(fid) => {
                if let Some(sig) = self.sigs.get(&(cx.mi, *fid)).cloned() {
                    self.direct_args(cx, cx.mi, &sig, args);
                    return vec![self.node(Key::Ret(cx.mi, *fid))];
                }
                self.call_args_unknown(cx, args);
                vec![]
            }
            Expr::ExternFuncRef { name, .. } => {
                if name == "js_eval_in_scope" {
                    for (i, a) in args.iter().enumerate() {
                        let v = self.expr(cx, a);
                        let r = if i == 1 || i == 2 {
                            Reason::EvalScope
                        } else {
                            Reason::IndirectCallArg
                        };
                        self.sink_vals(&v, r, "js_eval_in_scope");
                    }
                    return vec![];
                }
                if let Some((tm, fid)) = self.import_function(cx.mi, name, 0) {
                    if let Some(sig) = self.sigs.get(&(tm, fid)).cloned() {
                        self.direct_args(cx, tm, &sig, args);
                        return vec![self.node(Key::Ret(tm, fid))];
                    }
                }
                // A namespace callee (`ns()`) is a TypeError, not an
                // observation; an unresolved extern is untracked code.
                for a in args {
                    let v = self.expr(cx, a);
                    self.sink_vals(&v, Reason::IndirectCallArg, "extern");
                }
                vec![]
            }
            Expr::Closure {
                func_id,
                params,
                body,
                is_arrow,
                is_async,
                is_generator,
                ..
            } => {
                // An immediately invoked function expression is called, never
                // read as a value.
                self.entity(
                    Ent::Func(cx.mi, *func_id),
                    format!("closure#{func_id}"),
                    "closure",
                );
                let ret = (*is_async || *is_generator).then_some(Reason::AsyncReturn);
                self.body(
                    cx.mi,
                    *func_id,
                    params,
                    body,
                    ret,
                    *is_arrow && cx.static_this,
                );
                let sig = Self::sig_of(params);
                self.direct_args(cx, cx.mi, &sig, args);
                if ret.is_some() {
                    return vec![];
                }
                vec![self.node(Key::Ret(cx.mi, *func_id))]
            }
            Expr::PropertyGet {
                object, property, ..
            } => {
                let p = property.as_str();
                if p == "toString" || p == "toLocaleString" {
                    let v = self.expr(cx, object);
                    self.sink_vals(&v, Reason::ToStringCall, p);
                    self.call_args_unknown(cx, args);
                    return vec![];
                }
                if (p == "call" || p == "apply")
                    && matches!(object.as_ref(), Expr::PropertyGet { property, .. } if property == "toString")
                {
                    self.expr(cx, object);
                    for (i, a) in args.iter().enumerate() {
                        let v = self.expr(cx, a);
                        let r = if i == 0 {
                            Reason::FnProtoToStringCall
                        } else {
                            Reason::IndirectCallArg
                        };
                        self.sink_vals(&v, r, p);
                    }
                    return vec![];
                }
                let v = self.expr(cx, object);
                if !(CALL_PROPS.contains(&p) && !self.user_call_props) {
                    self.sink_vals(&v, Reason::MethodReceiver, p);
                }
                self.call_args_unknown(cx, args);
                vec![]
            }
            _ => {
                // Calling a value does not stringify it.
                self.expr(cx, callee);
                self.call_args_unknown(cx, args);
                vec![]
            }
        }
    }

    fn static_this(&mut self) -> u32 {
        self.node(Key::StaticThis)
    }

    // ---- expressions ----

    fn expr(&mut self, cx: Cx, e: &'a Expr) -> Vals {
        let mi = cx.mi;
        match e {
            Expr::FuncRef(fid) => {
                if self.sigs.contains_key(&(mi, *fid)) {
                    self.mark_valued(Ent::Func(mi, *fid));
                    vec![self.node(Key::Ent(Ent::Func(mi, *fid)))]
                } else {
                    vec![]
                }
            }
            Expr::Closure {
                func_id,
                params,
                body,
                is_arrow,
                is_async,
                is_generator,
                ..
            } => {
                let n = self.entity(
                    Ent::Func(mi, *func_id),
                    format!("closure#{func_id}"),
                    "closure",
                );
                self.mark_valued(Ent::Func(mi, *func_id));
                let ret = (*is_async || *is_generator).then_some(Reason::AsyncReturn);
                self.body(mi, *func_id, params, body, ret, *is_arrow && cx.static_this);
                vec![n]
            }
            Expr::ClassRef(name) => match self.resolve_name(mi, name) {
                Some(Resolved::Node(n)) => vec![n],
                Some(Resolved::Namespace(t)) => {
                    let module = self.modules[mi].0.clone();
                    self.note("namespace-import-read-as-value", || {
                        format!("{module}: {name}")
                    });
                    self.materialize_namespace(t);
                    vec![]
                }
                None => vec![],
            },
            Expr::ExternFuncRef { name, .. } => match self.resolve_name(mi, name) {
                Some(Resolved::Node(n)) => {
                    if let Some((tm, fid)) = self.import_function(mi, name, 0) {
                        self.mark_valued(Ent::Func(tm, fid));
                    }
                    vec![n]
                }
                Some(Resolved::Namespace(t)) => {
                    let module = self.modules[mi].0.clone();
                    self.note("namespace-import-read-as-value", || {
                        format!("{module}: {name}")
                    });
                    self.materialize_namespace(t);
                    vec![]
                }
                None => vec![],
            },
            Expr::LocalGet(id) => vec![self.local(mi, *id)],
            Expr::LocalSet(id, v) => {
                let vs = self.expr(cx, v);
                let l = self.local(mi, *id);
                self.flow(&vs, l);
                vec![l]
            }
            Expr::ScopedTemp { id, value, body } => {
                let vs = self.expr(cx, value);
                let l = self.local(mi, *id);
                self.flow(&vs, l);
                self.expr(cx, body)
            }
            Expr::GlobalGet(g) => {
                if self.global_ids[mi].contains(g) {
                    vec![self.node(Key::Global(mi, *g))]
                } else {
                    vec![]
                }
            }
            Expr::GlobalSet(g, v) => {
                let vs = self.expr(cx, v);
                if self.global_ids[mi].contains(g) {
                    let n = self.node(Key::Global(mi, *g));
                    self.flow(&vs, n);
                    vec![n]
                } else {
                    self.sink_vals(&vs, Reason::HeapStore, "global-object");
                    vec![]
                }
            }
            Expr::Update { id, .. } => {
                let l = self.local(mi, *id);
                self.sink_vals(&[l], Reason::ToPrimitive, "update");
                vec![]
            }
            Expr::Binary { left, right, .. } => {
                for x in [left, right] {
                    let v = self.expr(cx, x);
                    self.sink_vals(&v, Reason::ToPrimitive, "binary");
                }
                vec![]
            }
            Expr::Unary { op, operand } => {
                let v = self.expr(cx, operand);
                if !matches!(op, perry_hir::UnaryOp::Not) {
                    self.sink_vals(&v, Reason::ToPrimitive, "unary");
                }
                vec![]
            }
            Expr::Compare { op, left, right } => {
                let strict = matches!(op, perry_hir::CompareOp::Eq | perry_hir::CompareOp::Ne);
                for x in [left, right] {
                    let v = self.expr(cx, x);
                    if !strict {
                        self.sink_vals(&v, Reason::ToPrimitive, "compare");
                    }
                }
                vec![]
            }
            Expr::Logical { left, right, .. } => {
                let mut v = self.expr(cx, left);
                v.extend(self.expr(cx, right));
                v
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                self.expr(cx, condition);
                let mut v = self.expr(cx, then_expr);
                v.extend(self.expr(cx, else_expr));
                v
            }
            Expr::Sequence(es) => {
                let mut last = vec![];
                for x in es {
                    last = self.expr(cx, x);
                }
                last
            }
            Expr::TypeOf(x) | Expr::Void(x) | Expr::BooleanCoerce(x) => {
                self.expr(cx, x);
                vec![]
            }
            Expr::Await(x) => self.expr(cx, x),
            Expr::Yield { value, .. } => {
                if let Some(x) = value {
                    let v = self.expr(cx, x);
                    self.sink_vals(&v, Reason::Yield, "yield");
                }
                vec![]
            }
            Expr::Call { callee, args, .. } => self.call(cx, callee, args),
            // The CommonJS wrapper hands its factory to the runtime as the
            // last argument of the partial publication. The runtime's path
            // entry keeps it (no property) and only ever calls it
            // (`module_require.rs`), so the factory reaches no sink here. A
            // valued function's return already flows to the untracked-caller
            // sink. Every other argument stays a builtin sink.
            Expr::NativeMethodCall {
                module,
                method,
                object: None,
                args,
                ..
            } if module == "__perry_runtime" && method == "registerPathModulePartial" => {
                let factory = 2;
                let detail = format!("NativeMethodCall:{module}.{method}");
                for (i, a) in args.iter().enumerate() {
                    let v = self.expr(cx, a);
                    if i != factory {
                        self.sink_vals(&v, Reason::Builtin, &detail);
                    }
                }
                vec![]
            }
            Expr::CallSpread { callee, args, .. } => {
                self.expr(cx, callee);
                for a in args {
                    let x = match a {
                        CallArg::Expr(x) | CallArg::Spread(x) => x,
                    };
                    let v = self.expr(cx, x);
                    self.sink_vals(&v, Reason::IndirectCallArg, "spread-call");
                }
                vec![]
            }
            Expr::New {
                class_name, args, ..
            } => {
                let target = self.class_by_name(mi, class_name);
                match self.resolve_name(mi, class_name) {
                    Some(Resolved::Node(n)) => self.sink_vals(&[n], Reason::Instantiated, "new"),
                    Some(Resolved::Namespace(t)) => self.materialize_namespace(t),
                    None => {}
                }
                let ctor = target.and_then(|cid| self.ctor_sigs(mi, cid));
                match ctor {
                    Some(sig) => self.direct_args(cx, mi, &sig, args),
                    None => {
                        for a in args {
                            let v = self.expr(cx, a);
                            self.sink_vals(&v, Reason::HeapStore, "constructor-args");
                        }
                    }
                }
                vec![]
            }
            Expr::NewDynamic { callee, args, .. } => {
                let v = self.expr(cx, callee);
                self.sink_vals(&v, Reason::Instantiated, "new-dynamic");
                self.call_args_unknown(cx, args);
                vec![]
            }
            Expr::PropertyGet {
                object, property, ..
            } => {
                let v = self.expr(cx, object);
                if property == "prototype" {
                    self.sink_vals(&v, Reason::PrototypeRead, "prototype");
                }
                vec![]
            }
            Expr::PropertySet { object, value, .. } => {
                let o = self.expr(cx, object);
                self.sink_vals(&o, Reason::ErrorMessage, "property-write");
                let v = self.expr(cx, value);
                self.sink_vals(&v, Reason::HeapStore, "property");
                vec![]
            }
            Expr::PropertyUpdate { object, .. } => {
                let o = self.expr(cx, object);
                self.sink_vals(&o, Reason::ErrorMessage, "property-update");
                vec![]
            }
            Expr::IndexGet { object, index, .. } => {
                self.expr(cx, object);
                let k = self.expr(cx, index);
                self.sink_vals(&k, Reason::ToPropertyKey, "index");
                vec![]
            }
            Expr::IndexSet {
                object,
                index,
                value,
                ..
            } => {
                let o = self.expr(cx, object);
                self.sink_vals(&o, Reason::ErrorMessage, "index-write");
                let k = self.expr(cx, index);
                self.sink_vals(&k, Reason::ToPropertyKey, "index");
                let v = self.expr(cx, value);
                self.sink_vals(&v, Reason::HeapStore, "index");
                vec![]
            }
            Expr::IndexUpdate { object, index, .. } => {
                let o = self.expr(cx, object);
                self.sink_vals(&o, Reason::ErrorMessage, "index-update");
                let k = self.expr(cx, index);
                self.sink_vals(&k, Reason::ToPropertyKey, "index");
                vec![]
            }
            Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            } => {
                for x in [target, receiver] {
                    let o = self.expr(cx, x);
                    self.sink_vals(&o, Reason::ErrorMessage, "put-value");
                }
                let k = self.expr(cx, key);
                self.sink_vals(&k, Reason::ToPropertyKey, "put-value");
                let v = self.expr(cx, value);
                self.sink_vals(&v, Reason::HeapStore, "put-value");
                vec![]
            }
            Expr::Object(props) => {
                for (_, x) in props {
                    let v = self.expr(cx, x);
                    self.sink_vals(&v, Reason::HeapStore, "object-literal");
                }
                vec![]
            }
            Expr::ObjectSpread { parts } => {
                for (key, x) in parts {
                    let v = self.expr(cx, x);
                    if key.is_some() {
                        self.sink_vals(&v, Reason::HeapStore, "object-literal");
                    }
                }
                vec![]
            }
            Expr::Array(elems, _) => {
                for x in elems {
                    let v = self.expr(cx, x);
                    self.sink_vals(&v, Reason::HeapStore, "array-literal");
                }
                vec![]
            }
            Expr::ArraySpread(elems) => {
                for el in elems {
                    match el {
                        ArrayElement::Expr(x) => {
                            let v = self.expr(cx, x);
                            self.sink_vals(&v, Reason::HeapStore, "array-literal");
                        }
                        ArrayElement::Spread(x) => {
                            let v = self.expr(cx, x);
                            self.sink_vals(&v, Reason::Builtin, "array-spread-iterate");
                        }
                        ArrayElement::Hole => {}
                    }
                }
                vec![]
            }
            Expr::This => {
                if cx.static_this {
                    vec![self.static_this()]
                } else {
                    vec![]
                }
            }
            Expr::NewTarget => vec![self.static_this()],
            Expr::StaticMethodCall {
                class_name,
                method_name,
                args,
            } => {
                let target = self.class_by_name(mi, class_name).and_then(|_| {
                    self.static_sigs
                        .get(&(mi, class_name.clone(), method_name.clone()))
                        .copied()
                });
                match target.and_then(|fid| self.sigs.get(&(mi, fid)).cloned()) {
                    Some(sig) => self.direct_args(cx, mi, &sig, args),
                    None => self.call_args_unknown(cx, args),
                }
                vec![]
            }
            Expr::InstanceOf { expr, .. } => {
                let v = self.expr(cx, expr);
                if self.has_instance_hook {
                    self.sink_vals(&v, Reason::InstanceofHook, "instanceof");
                }
                // The dynamic right-hand side is only called through
                // Symbol.hasInstance; walk it for nested closures.
                perry_hir::walker::walk_expr_children(e, &mut |c| {
                    if !std::ptr::eq(c, expr.as_ref()) {
                        self.expr(cx, c);
                    }
                });
                vec![]
            }
            Expr::In { property, object } => {
                let k = self.expr(cx, property);
                self.sink_vals(&k, Reason::ToPropertyKey, "in");
                self.expr(cx, object);
                vec![]
            }
            Expr::Delete(x) => {
                match x.as_ref() {
                    Expr::PropertyGet { object, .. } => {
                        let o = self.expr(cx, object);
                        self.sink_vals(&o, Reason::ErrorMessage, "delete");
                    }
                    Expr::IndexGet { object, index, .. } => {
                        let o = self.expr(cx, object);
                        self.sink_vals(&o, Reason::ErrorMessage, "delete");
                        let k = self.expr(cx, index);
                        self.sink_vals(&k, Reason::ToPropertyKey, "delete");
                    }
                    other => {
                        self.expr(cx, other);
                    }
                }
                vec![]
            }
            Expr::WithGet {
                object, fallback, ..
            } => {
                let o = self.expr(cx, object);
                self.sink_vals(&o, Reason::With, "with");
                self.expr(cx, fallback)
            }
            Expr::DynamicImport {
                paths,
                arg,
                options,
                deferred_error,
                ..
            } => {
                let v = self.expr(cx, arg);
                self.sink_vals(&v, Reason::ToStringBuiltin, "import-specifier");
                if let Some(o) = options {
                    let v = self.expr(cx, o);
                    self.sink_vals(&v, Reason::IndirectCallArg, "import-options");
                }
                if deferred_error.is_none() {
                    let targets: Vec<Option<usize>> =
                        paths.iter().map(|p| self.target(mi, p)).collect();
                    // A bare specifier the resolver did not link (`http`,
                    // `node:fs`) names a builtin, not a compiled module.
                    let file_like = |p: &str| p.starts_with('/') || p.starts_with('.');
                    let unknown_file = paths
                        .iter()
                        .zip(&targets)
                        .any(|(p, t)| t.is_none() && file_like(p));
                    if paths.is_empty() || unknown_file {
                        let module = self.modules[mi].0.clone();
                        let unresolved: Vec<String> = paths
                            .iter()
                            .zip(&targets)
                            .filter(|(_, t)| t.is_none())
                            .map(|(p, _)| p.clone())
                            .collect();
                        self.note(
                            if paths.is_empty() {
                                "dynamic-import-no-paths"
                            } else {
                                "dynamic-import-unresolved-path"
                            },
                            || format!("{module} -> {unresolved:?}"),
                        );
                        for t in 0..self.modules.len() {
                            self.materialize_namespace(t);
                        }
                    }
                    for t in targets.into_iter().flatten() {
                        self.materialize_namespace(t);
                    }
                }
                vec![]
            }
            _ => self.unmodeled(cx, e),
        }
    }

    fn unmodeled(&mut self, cx: Cx, e: &'a Expr) -> Vals {
        let mut held = vec![];
        perry_hir::walker::walk_expr_children(e, &mut |c| {
            let v = self.expr(cx, c);
            held.extend(v);
        });
        if !held.is_empty() {
            let name = variant_name(e);
            let reason = builtin_reason(&name, e);
            let detail = match e {
                Expr::NativeMethodCall { module, method, .. } => {
                    format!("{name}:{module}.{method}")
                }
                _ => name,
            };
            self.sink_vals(&held, reason, &detail);
        }
        vec![]
    }

    fn ctor_sigs(&self, mi: usize, cid: u32) -> Option<Sig> {
        self.modules[mi]
            .1
            .classes
            .iter()
            .find(|c| c.id == cid)
            .and_then(|c| c.constructor.as_ref())
            .map(|f| Self::sig_of(&f.params))
    }

    // ---- classes ----

    fn class(&mut self, mi: usize, c: &'a Class) {
        let cn = self.node(Key::Ent(Ent::Class(mi, c.id)));
        let module_cx = Cx {
            mi,
            func: None,
            ret_sink: None,
            static_this: false,
        };
        let static_cx = Cx {
            static_this: true,
            ..module_cx
        };
        if let Some(parent) = &c.extends_name {
            match self.resolve_name(mi, parent) {
                Some(Resolved::Node(p)) => self.edge(p, cn),
                Some(Resolved::Namespace(t)) => self.materialize_namespace(t),
                None => {}
            }
        }
        if let Some(e) = &c.extends_expr {
            let v = self.expr(module_cx, e);
            self.flow(&v, cn);
        }
        let has_static = !c.static_methods.is_empty()
            || !c.static_fields.is_empty()
            || !c.static_accessor_fn_ids.is_empty()
            || c.computed_members.iter().any(|m| m.is_static)
            || c.extends_name.is_some()
            || c.extends_expr.is_some();
        if has_static {
            let st = self.static_this();
            self.edge(cn, st);
        }
        for d in &c.decorators {
            self.sink_vals(&[cn], Reason::Decorator, &d.name);
            self.call_args_unknown(module_cx, &d.args);
        }
        if let Some(f) = &c.constructor {
            self.function(mi, f, Some(Reason::ReturnFromValueFn), false);
        }
        let member = |b: &mut Self, f: &'a Function, kind: &'static str, is_static: bool| {
            let n = b.entity(Ent::Func(mi, f.id), format!("{}.{}", c.name, f.name), kind);
            b.mark_valued(Ent::Func(mi, f.id));
            b.sink_vals(&[n], Reason::ClassMember, kind);
            b.function(mi, f, Some(Reason::ReturnFromValueFn), is_static);
        };
        for f in &c.methods {
            member(self, f, "method", false);
        }
        for f in &c.static_methods {
            member(self, f, "static-method", true);
        }
        for (_, f) in c.getters.iter().chain(&c.setters) {
            let st = c.static_accessor_fn_ids.contains(&f.id);
            member(self, f, "accessor", st);
        }
        for m in &c.computed_members {
            let cx = if m.is_static { static_cx } else { module_cx };
            let k = self.expr(cx, &m.key_expr);
            self.sink_vals(&k, Reason::ToPropertyKey, "computed-member-key");
            member(self, &m.function, "computed-member", m.is_static);
        }
        for (fields, is_static) in [(&c.fields, false), (&c.static_fields, true)] {
            let cx = if is_static { static_cx } else { module_cx };
            for f in fields {
                if let Some(k) = &f.key_expr {
                    let v = self.expr(cx, k);
                    self.sink_vals(&v, Reason::ToPropertyKey, "field-key");
                }
                if let Some(i) = &f.init {
                    let v = self.expr(cx, i);
                    self.sink_vals(&v, Reason::HeapStore, "field");
                }
                for d in &f.decorators {
                    self.call_args_unknown(cx, &d.args);
                }
            }
        }
    }
}

/// The HIR variant name, read from the start of its `Debug` text without
/// formatting the whole subtree.
fn variant_name(e: &Expr) -> String {
    struct Cap(String);
    impl std::fmt::Write for Cap {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.0.push_str(s);
            if self.0.len() >= 48 {
                Err(std::fmt::Error)
            } else {
                Ok(())
            }
        }
    }
    let mut cap = Cap(String::new());
    let _ = std::fmt::write(&mut cap, format_args!("{e:?}"));
    cap.0
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

fn builtin_reason(name: &str, e: &Expr) -> Reason {
    const STRINGIFY: &[&str] = &[
        "StringCoerce",
        "TemplateStringCoerce",
        "NumberCoerce",
        "BigIntCoerce",
        "ParseInt",
        "ParseFloat",
        "EncodeURI",
        "EncodeURIComponent",
        "DecodeURI",
        "DecodeURIComponent",
        "ErrorNew",
        "ErrorNewWithCause",
        "ErrorNewWithOptions",
        "TypeErrorNew",
        "RangeErrorNew",
        "ReferenceErrorNew",
        "SyntaxErrorNew",
        "AggregateErrorNew",
        "RegExpDynamic",
        "ArrayJoin",
        "SymbolFor",
        "SymbolNew",
        "StringRaw",
        "IsNaN",
        "IsFinite",
        "MathFloor",
        "MathCeil",
        "MathRound",
        "MathAbs",
        "MathMin",
        "MathMax",
        "Atob",
        "Btoa",
    ];
    if STRINGIFY.contains(&name) {
        return Reason::ToStringBuiltin;
    }
    if name.starts_with("Proxy") || name.starts_with("Reflect") {
        return Reason::Reflection;
    }
    if name == "WorkerNew" {
        return Reason::Worker;
    }
    if let Expr::NativeMethodCall { module, .. } = e {
        if module == "perry/thread" || module == "worker_threads" || module.contains("thread") {
            return Reason::Worker;
        }
    }
    if name.starts_with("WithSet") {
        return Reason::With;
    }
    Reason::Builtin
}

/// An exported name only a type, interface or enum declaration binds holds
/// no function value.
fn type_only_name(m: &Module, name: &str) -> bool {
    m.type_aliases.iter().any(|t| t.name == name)
        || m.interfaces.iter().any(|i| i.name == name)
        || m.enums.iter().any(|e| e.name == name)
        || m.imports.iter().any(|i| {
            i.type_only
                && i.specifiers.iter().any(|s| match s {
                    perry_hir::ImportSpecifier::Named { local, .. }
                    | perry_hir::ImportSpecifier::Default { local }
                    | perry_hir::ImportSpecifier::Namespace { local } => local == name,
                })
        })
}

fn module_has_instance_hook(m: &Module) -> bool {
    m.functions
        .iter()
        .any(|f| f.name.starts_with("__perry_wk_hasinstance_"))
}

fn defines_call_props(m: &Module) -> bool {
    let named = |n: &str| CALL_PROPS.contains(&n);
    if m.classes.iter().any(|c| {
        c.methods
            .iter()
            .chain(&c.static_methods)
            .any(|f| named(&f.name))
            || c.fields
                .iter()
                .chain(&c.static_fields)
                .any(|f| named(&f.name))
            || c.getters.iter().any(|(n, _)| named(n))
    }) {
        return true;
    }
    let mut found = false;
    let mut check = |e: &Expr| {
        match e {
            Expr::PropertySet { property, .. } if named(property) => found = true,
            Expr::Object(props) if props.iter().any(|(k, _)| named(k)) => found = true,
            Expr::ObjectSpread { parts }
                if parts.iter().any(|(k, _)| k.as_deref().is_some_and(named)) =>
            {
                found = true
            }
            _ => {}
        }
        found
    };
    perry_hir::for_each_module_expr(m, &mut |e| {
        check(e);
    });
    found
}

impl Need {
    pub fn compute(
        modules: &[(String, &Module)],
        links: &BTreeMap<(String, String), String>,
        entry: &str,
        library: bool,
        dynamic: &perry_hir::dynamic_code::DynamicCodeReach,
    ) -> Self {
        let paths: FxHashMap<&str, usize> = modules
            .iter()
            .enumerate()
            .map(|(i, (p, _))| (p.as_str(), i))
            .collect();
        let mut module_links: Vec<FxHashMap<&str, usize>> =
            vec![FxHashMap::default(); modules.len()];
        for ((importer, source), target) in links {
            if let (Some(&mi), Some(&ti)) =
                (paths.get(importer.as_str()), paths.get(target.as_str()))
            {
                module_links[mi].insert(source.as_str(), ti);
            }
        }
        let mut b = Build {
            modules,
            links: module_links,
            paths,
            keys: FxHashMap::default(),
            edges: Vec::new(),
            sinks: Vec::new(),
            sink_ids: vec![FxHashMap::default(); REASONS.len()],
            export_names: Vec::new(),
            export_ids: vec![FxHashMap::default(); modules.len()],
            module_exports: vec![Vec::new(); modules.len()],
            entities: Vec::new(),
            ent_index: FxHashMap::default(),
            valued: FxHashSet::default(),
            sigs: FxHashMap::default(),
            static_sigs: FxHashMap::default(),
            fn_names: vec![FxHashMap::default(); modules.len()],
            class_names: vec![FxHashMap::default(); modules.len()],
            global_names: vec![FxHashMap::default(); modules.len()],
            global_ids: vec![FxHashSet::default(); modules.len()],
            init_locals: vec![FxHashMap::default(); modules.len()],
            namespaces_materialized: BTreeSet::new(),
            user_call_props: modules.iter().any(|(_, m)| defines_call_props(m)),
            has_instance_hook: modules.iter().any(|(_, m)| module_has_instance_hook(m)),
            edge_count: 0,
            notes: BTreeMap::new(),
            samples: Vec::new(),
            unresolved_export_modules: BTreeSet::new(),
            relabel: FxHashMap::default(),
        };
        // Pre-pass: names, signatures and entities of declarations.
        for (mi, (_, m)) in modules.iter().enumerate() {
            for f in &m.functions {
                b.fn_names[mi].insert(f.name.as_str(), f.id);
                b.sigs.insert((mi, f.id), Build::sig_of(&f.params));
                b.entity(Ent::Func(mi, f.id), f.name.clone(), "function");
            }
            for c in &m.classes {
                b.class_names[mi].insert(c.name.as_str(), c.id);
                for a in &c.aliases {
                    b.class_names[mi].insert(a.as_str(), c.id);
                }
                b.entity(Ent::Class(mi, c.id), format!("class {}", c.name), "class");
                for f in &c.static_methods {
                    b.sigs.insert((mi, f.id), Build::sig_of(&f.params));
                    b.static_sigs
                        .insert((mi, c.name.clone(), f.name.clone()), f.id);
                }
            }
            for g in &m.globals {
                b.global_names[mi].insert(g.name.as_str(), g.id);
                b.global_ids[mi].insert(g.id);
            }
            for s in &m.init {
                if let Stmt::Let { id, name, .. } = s {
                    b.init_locals[mi].insert(name.as_str(), *id);
                }
            }
        }
        // Bodies.
        for (mi, (path, m)) in modules.iter().enumerate() {
            let cx = Cx {
                mi,
                func: None,
                ret_sink: None,
                static_this: false,
            };
            for g in &m.globals {
                if let Some(e) = &g.init {
                    let v = b.expr(cx, e);
                    let n = b.node(Key::Global(mi, g.id));
                    b.flow(&v, n);
                }
            }
            b.stmts(cx, &m.init);
            for f in &m.functions {
                b.function(mi, f, None, false);
            }
            for c in &m.classes {
                b.class(mi, c);
            }
            for (_, fid) in &m.script_global_functions {
                let n = b.node(Key::Ent(Ent::Func(mi, *fid)));
                b.sink_vals(&[n], Reason::GlobalPublication, "script-global");
            }
            let lexical =
                dynamic.reaches_module_bindings(path) || dynamic.reaches_module_bindings(&m.name);
            if dynamic.globals || lexical {
                let globals: Vec<u32> = m
                    .globals
                    .iter()
                    .map(|g| b.node(Key::Global(mi, g.id)))
                    .collect();
                b.sink_vals(&globals, Reason::DynCodeGlobals, "module-global");
            }
            if lexical {
                let mut held: Vec<u32> = m
                    .functions
                    .iter()
                    .map(|f| b.node(Key::Ent(Ent::Func(mi, f.id))))
                    .collect();
                let locals: Vec<u32> = b.init_locals[mi].values().copied().collect();
                for l in locals {
                    held.push(b.local(mi, l));
                }
                for c in &m.classes {
                    held.push(b.node(Key::Ent(Ent::Class(mi, c.id))));
                }
                b.sink_vals(&held, Reason::DirectEvalModule, "direct-eval");
            }
        }
        // Export bindings.
        for (mi, (path, m)) in modules.iter().enumerate() {
            for (name, fid) in &m.exported_functions {
                let x = b.export_node(mi, name);
                let n = b.node(Key::Ent(Ent::Func(mi, *fid)));
                b.edge(n, x);
                // Outside callers of an export receive what it returns.
                let r = b.node(Key::Ret(mi, *fid));
                b.edge(r, x);
            }
            for e in &m.exports {
                match e {
                    perry_hir::Export::Named { local, exported } => {
                        let x = b.export_node(mi, exported);
                        match b.resolve_name(mi, local) {
                            Some(Resolved::Node(n)) => b.edge(n, x),
                            Some(Resolved::Namespace(t)) => b.materialize_namespace(t),
                            None if type_only_name(modules[mi].1, local) => {}
                            None => {
                                // A binding this pass cannot name: whatever
                                // the module holds may be behind it.
                                let module = modules[mi].0.clone();
                                b.note("export-local-unresolved", || format!("{module}: {local}"));
                                b.unresolved_export_modules.insert(mi);
                            }
                        }
                    }
                    perry_hir::Export::ReExport {
                        source,
                        imported,
                        exported,
                    } => {
                        if let Some(t) = b.target(mi, source) {
                            let from = b.export_node(t, imported);
                            let x = b.export_node(mi, exported);
                            b.edge(from, x);
                        }
                    }
                    perry_hir::Export::NamespaceReExport { source, name } => {
                        b.export_node(mi, name);
                        if let Some(t) = b.target(mi, source) {
                            let module = modules[mi].0.clone();
                            b.note("namespace-re-export", || format!("{module}: {name}"));
                            b.materialize_namespace(t);
                        }
                    }
                    perry_hir::Export::ExportAll { .. } => {}
                }
            }
            let _ = path;
        }
        for mi in b.unresolved_export_modules.clone() {
            let m = modules[mi].1;
            let mut held: Vec<u32> = m
                .globals
                .iter()
                .map(|g| b.node(Key::Global(mi, g.id)))
                .collect();
            let locals: Vec<u32> = b.init_locals[mi].values().copied().collect();
            for l in locals {
                held.push(b.local(mi, l));
            }
            for f in &m.functions {
                held.push(b.node(Key::Ent(Ent::Func(mi, f.id))));
            }
            for c in &m.classes {
                held.push(b.node(Key::Ent(Ent::Class(mi, c.id))));
            }
            b.sink_vals(&held, Reason::LibraryExport, "unresolved-export-binding");
        }
        // Star re-exports: a binding visible through `export *` is the source
        // module's binding. Iterate until no new names appear.
        loop {
            let before = b.export_names.len();
            for (mi, (_, m)) in modules.iter().enumerate() {
                for e in &m.exports {
                    if let perry_hir::Export::ExportAll { source } = e {
                        if let Some(t) = b.target(mi, source) {
                            let names: Vec<String> = b.module_exports[t]
                                .iter()
                                .map(|&id| &b.export_names[id as usize].1)
                                .filter(|n| *n != "default")
                                .cloned()
                                .collect();
                            for n in names {
                                let explicit = b.export_ids[mi].contains_key(n.as_str());
                                let from = b.export_node(t, &n);
                                let x = b.export_node(mi, &n);
                                if !explicit {
                                    b.edge(from, x);
                                }
                            }
                        }
                    }
                }
            }
            if b.export_names.len() == before {
                break;
            }
        }
        for id in 0..b.export_names.len() {
            let (mi, _) = b.export_names[id].clone();
            let x = b.node(Key::Export(mi, id as u32));
            let reason = if library && modules[mi].0 == entry {
                Reason::LibraryExport
            } else {
                Reason::Export
            };
            b.sink_vals(&[x], reason, "export");
            if b.namespaces_materialized.contains(&mi) {
                b.sink_vals(&[x], Reason::NamespaceObject, "namespace");
            }
        }
        // A function that is ever a value can be called by untracked code,
        // which receives whatever it returns.
        let valued: Vec<usize> = b.valued.iter().copied().collect();
        for e in valued {
            if let Ent::Func(mi, fid) = b.entities[e].ent {
                let r = b.node(Key::Ret(mi, fid));
                b.sink_vals(&[r], Reason::ReturnFromValueFn, "returned");
            }
        }
        b.solve(modules)
    }
}

impl<'a> Build<'a> {
    fn solve(mut self, modules: &[(String, &Module)]) -> Need {
        for info in &mut self.entities {
            if let Some(kind) = self.relabel.get(&info.ent) {
                info.kind = kind;
            }
        }
        let n = self.edges.len();
        let mut reverse: Vec<Vec<u32>> = vec![Vec::new(); n];
        for (from, tos) in self.edges.iter().enumerate() {
            for &to in tos {
                reverse[to as usize].push(from as u32);
            }
        }
        let mut sink_nodes: Vec<(Reason, String, u32)> = self
            .sinks
            .iter()
            .enumerate()
            .filter_map(|(id, (r, d))| {
                self.keys
                    .get(&Key::Sink(id as u32))
                    .map(|&node| (*r, d.clone(), node))
            })
            .collect();
        sink_nodes.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        let mut mask = vec![0u64; n];
        for reason in REASONS {
            let mut seen = vec![false; n];
            let mut work: VecDeque<u32> = sink_nodes
                .iter()
                .filter(|s| s.0 == reason)
                .map(|s| s.2)
                .collect();
            for &w in &work {
                seen[w as usize] = true;
            }
            while let Some(x) = work.pop_front() {
                mask[x as usize] |= reason.bit();
                for &p in &reverse[x as usize] {
                    if !seen[p as usize] {
                        seen[p as usize] = true;
                        work.push_back(p);
                    }
                }
            }
        }
        // The runtime prints source in a property-write error only for a
        // class ("Cannot assign to read only property .. of function
        // '<class source>'"), so that sink does not count for function
        // owners: their witness is computed without it.
        let witness_with = |skip: Option<Reason>| {
            let mut witness: Vec<Option<u32>> = vec![None; n];
            for (i, (r, _, node)) in sink_nodes.iter().enumerate() {
                if Some(*r) == skip || witness[*node as usize].is_some() {
                    continue;
                }
                let mut work = VecDeque::from([*node]);
                witness[*node as usize] = Some(i as u32);
                while let Some(x) = work.pop_front() {
                    for &p in &reverse[x as usize] {
                        if witness[p as usize].is_none() {
                            witness[p as usize] = Some(i as u32);
                            work.push_back(p);
                        }
                    }
                }
            }
            witness
        };
        let class_witness = witness_with(None);
        let func_witness = witness_with(Some(Reason::ErrorMessage));
        let ents = self.entities.len();
        let mut emask = vec![0u64; ents];
        let mut ewit = vec![None; ents];
        let mut evalued = vec![false; ents];
        for (e, info) in self.entities.iter().enumerate() {
            if let Some(&node) = self.keys.get(&Key::Ent(info.ent)) {
                let (witness, keep) = match info.ent {
                    Ent::Class(..) => (&class_witness, !0u64),
                    Ent::Func(..) => (&func_witness, !Reason::ErrorMessage.bit()),
                };
                emask[e] = mask[node as usize] & keep;
                ewit[e] = witness[node as usize].map(|i| {
                    let (r, d, _) = &sink_nodes[i as usize];
                    (*r, d.clone())
                });
                evalued[e] = !self.edges[node as usize].is_empty();
            }
            evalued[e] |= self.valued.contains(&e);
        }
        Need {
            entities: self.entities,
            index: self.ent_index,
            mask: emask,
            valued: evalued,
            witness: ewit,
            module_names: modules.iter().map(|(_, m)| m.name.clone()).collect(),
            module_paths: modules.iter().map(|(p, _)| p.clone()).collect(),
            nodes: n,
            edges: self.edge_count,
            notes: self.notes,
            samples: self.samples,
        }
    }
}
