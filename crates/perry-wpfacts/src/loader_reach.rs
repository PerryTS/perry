//! Reachability of loader capabilities over the completed inclusion graph.
//! This reads solved points-to/call relations; it adds no solve constraints.
use crate::gen::{Fx, Gen};
use crate::model::*;
use crate::pts::{FxHashMap, FxHashSet};
use perry_hir::Expr;

#[derive(Clone, Debug)]
pub(crate) enum Op {
    Root,
    Value(NodeId, bool),
    Reference(NodeId, u8),
    Accessor(Tok, FnIdx),
    Module(u32),
    LoaderCall(NodeId, Option<String>),
    Function(FnIdx),
    Prototype(NodeId),
    Call(CallId, bool, bool),
    Access(NodeId, Key, bool, bool),
    Index(NodeId, NodeId, bool, bool),
    Touch(NodeId),
    Token(Tok),
    Escape(NodeId),
    Capability(String),
}
impl Op {
    pub(crate) fn constraint(
        n: NodeId,
        c: Cons,
        user: bool,
        index: Option<NodeId>,
    ) -> Option<Self> {
        Some(match c {
            Cons::Load { key: Key::Any, .. } | Cons::MCall { key: Key::Any, .. }
                if index.is_some() =>
            {
                Self::Index(n, index.unwrap(), user, false)
            }
            Cons::Store { key: Key::Any, .. } if index.is_some() => {
                Self::Index(n, index.unwrap(), user, true)
            }
            Cons::Load { key, .. } | Cons::MCall { key, .. } => Self::Access(n, key, false, user),
            Cons::Store { key, .. } => Self::Access(n, key, true, user),
            Cons::IdxLoad { idx, .. } => Self::Index(n, idx, user, false),
            Cons::Implicit { .. } if user => Self::Touch(n),
            Cons::TopSink { .. } if user => Self::Escape(n),
            _ => return None,
        })
    }
}
pub(crate) fn native_capability(s: &str) -> bool {
    matches!(
        s.strip_prefix("node:").unwrap_or(s),
        "module" | "vm" | "repl" | "inspector" | "inspector/promises" | "bun:ffi"
    ) || s.ends_with(".node")
        || s.starts_with("data:")
}
impl Gen<'_> {
    pub(crate) fn loader_source_origin(&self, _mi: u32, e: &Expr) -> Option<bool> {
        match e {
            Expr::CompilerOwned(_) => Some(false),
            Expr::Call { byte_offset, .. }
            | Expr::PropertyGet { byte_offset, .. }
            | Expr::DynamicImport { byte_offset, .. }
                if *byte_offset != 0 =>
            {
                Some(byte_offset & perry_hir::SOURCE_COMPILER_OWNED == 0)
            }
            Expr::IndexGet { object, .. }
            | Expr::IndexSet { object, .. }
            | Expr::PropertySet { object, .. } => self.loader_source_origin(_mi, object),
            _ => None,
        }
    }
    pub(crate) fn note_loader_expr(&mut self, fx: &Fx, e: &Expr) {
        if !self.m.reach_user {
            return;
        }
        match e {
            Expr::ClassExprFresh { template, .. } | Expr::ClassRef(template) => {
                if let Some(tm) = self.mods[fx.module as usize].classes.get(template) {
                    if let Some(&f) = self.static_init_fn.get(tm) {
                        self.m.reach_ops.push((fx.f, Op::Function(f)));
                    }
                }
            }
            _ => {}
        }
        let imports = &self.modules[fx.module as usize].1.imports;
        let read_name = match e {
            Expr::ExternFuncRef { name, .. } | Expr::ClassRef(name) => Some(name),
            _ => None,
        };
        if let Some(name) = read_name {
            for import in imports {
                if import.specifiers.iter().any(|sp| match sp {
                    perry_hir::ImportSpecifier::Named { local, .. }
                    | perry_hir::ImportSpecifier::Default { local }
                    | perry_hir::ImportSpecifier::Namespace { local } => local == name,
                }) {
                    if let Some(path) = &import.resolved_path {
                        if let Some(&mi) = self.path_index.get(path) {
                            self.m.reach_ops.push((fx.f, Op::Module(mi)));
                        }
                    }
                    if native_capability(&import.source) {
                        self.m.reach_ops.push((
                            fx.f,
                            Op::Capability(format!("loader import {}", import.source)),
                        ));
                    }
                }
            }
        }
        if let Expr::DynamicImport { paths, .. } = e {
            for path in paths {
                if let Some(&mi) = self.path_index.get(path) {
                    self.m.reach_ops.push((fx.f, Op::Module(mi)));
                }
            }
        }
        if let Expr::Call { callee, args, .. } = e {
            if let Expr::LocalGet(id) = callee.as_ref() {
                let literal = match args.as_slice() {
                    [Expr::String(s)] => Some(s.clone()),
                    _ => None,
                };
                if literal.as_deref().is_none_or(native_capability) {
                    let n = self.local(fx.module, *id);
                    self.m.reach_ops.push((fx.f, Op::LoaderCall(n, literal)));
                }
            }
        }
        let evaluator = match e {
            Expr::ExternFuncRef { name, .. } | Expr::ClassRef(name) => Some(name.as_str()),
            _ => None,
        };
        if let Some(name @ ("Function" | "eval")) = evaluator {
            let mi = &self.mods[fx.module as usize];
            if !mi.scope.contains_key(name)
                && !mi.imports.contains_key(name)
                && !mi.classes.contains_key(name)
            {
                self.m
                    .reach_ops
                    .push((fx.f, Op::Capability(format!("global evaluator {name}"))));
            }
        }
        if let Expr::Call { callee, .. } | Expr::CallSpread { callee, .. } = e {
            if let Expr::ExternFuncRef { name, .. } = callee.as_ref() {
                if matches!(
                    name.as_str(),
                    "js_eval"
                        | "js_eval_in_scope"
                        | "js_function_ctor_from_strings"
                        | "js_vm_module_call"
                        | "js_module_create_require_devirt"
                ) {
                    self.m
                        .reach_ops
                        .push((fx.f, Op::Capability(format!("dynamic/loader call {name}"))));
                }
            }
        }
        let why = match e {
            Expr::NativeModuleRef(s) if native_capability(s) => {
                Some(format!("native loader module {s}"))
            }
            Expr::NativeMethodCall { module, method, .. }
                if matches!(module.as_str(), "bun" | "deno")
                    && matches!(method.as_str(), "dlopen" | "plugin") =>
            {
                Some(format!("native loader operation {module}.{method}"))
            }
            Expr::NativeMethodCall {
                module,
                method,
                args,
                ..
            } if module.strip_prefix("node:").unwrap_or(module) == "process"
                && method == "getBuiltinModule"
                && !matches!(args.as_slice(), [Expr::String(s)] if !native_capability(s)) =>
            {
                Some("process.getBuiltinModule may return a loader".into())
            }
            Expr::NativeMethodCall { module, method, .. }
                if module.strip_prefix("node:").unwrap_or(module) == "process"
                    && matches!(method.as_str(), "binding" | "_linkedBinding" | "dlopen") =>
            {
                Some(format!("process.{method} loader entry"))
            }
            Expr::NativeMethodCall { module, method, .. } if native_capability(module) => {
                Some(format!("native loader operation {module}.{method}"))
            }
            Expr::New { class_name, .. } if class_name == "Function" => {
                Some("deferred Function constructor".into())
            }
            Expr::DynamicImport { arg, .. } if !matches!(arg.as_ref(), Expr::String(s) if !native_capability(s)) => {
                Some("non-constant dynamic import".into())
            }
            Expr::ExternFuncRef { name, .. }
                if matches!(
                    name.as_str(),
                    "js_eval_in_scope"
                        | "js_eval"
                        | "js_function_ctor_from_strings"
                        | "js_module_create_require_devirt"
                        | "js_vm_module_call"
                ) =>
            {
                Some(format!("dynamic/loader entry {name}"))
            }
            _ => None,
        };
        if let Some(why) = why {
            self.m.reach_ops.push((fx.f, Op::Capability(why)));
        }
    }
}

struct Reach<'a> {
    g: &'a Gen<'a>,
    ops: Vec<Vec<&'a Op>>,
    calls: Vec<Vec<Tok>>,
    live: FxHashSet<FnIdx>,
    queue: std::collections::VecDeque<FnIdx>,
    touched: FxHashSet<Tok>,
    modules: FxHashSet<u32>,
    init: Vec<Option<FnIdx>>,
    extra_accessors: FxHashMap<Tok, Vec<FnIdx>>,
    refusal: Option<String>,
    context: FnIdx,
    children: Vec<Vec<CallId>>,
    active_calls: FxHashSet<CallId>,
    call_ops: Vec<Vec<&'a Op>>,
    protocols: FxHashMap<Tok, Vec<CallId>>,
    top_reached: bool,
    in_top_graph: bool,
    private_arguments: FxHashSet<Tok>,
}
fn possible_loader_member(name: Option<&str>) -> bool {
    name.is_none_or(|n| {
        matches!(
            n,
            "Function"
                | "eval"
                | "module"
                | "require"
                | "Bun"
                | "Deno"
                | "cache"
                | "_cache"
                | "mainModule"
                | "getBuiltinModule"
                | "createRequire"
                | "binding"
                | "_linkedBinding"
                | "dlopen"
                | "constructor"
                | "caller"
                | "callee"
                | "extensions"
                | "_extensions"
                | "_compile"
                | "_load"
        ) || n.starts_with("__perry_cjs_")
            || n.starts_with("__cjs_")
    })
}
impl<'a> Reach<'a> {
    // TOP is shared with compiler bookkeeping in the invariance solve.
    // Private loader state is acquired only through the guarded original
    // source references/property paths, not by that summary pollution.
    fn private_token(&self, t: Tok, seen: &mut FxHashSet<Tok>) -> bool {
        if !seen.insert(t) {
            return true;
        }
        match self.g.m.toks[t as usize] {
            TokKind::Func(f) => self.g.m.funcs[f as usize].compiler_owned,
            TokKind::Bound(b) => {
                let p = &self.g.m.nodes[self.g.m.bound[b as usize].0 as usize].pts;
                !p.is_empty()
                    && !p.contains_tok(TOP)
                    && p.to_sorted()
                        .into_iter()
                        .all(|t| self.private_token(t, seen))
            }
            _ => self.module_record(t),
        }
    }
    fn factory_token(&self, t: Tok, seen: &mut FxHashSet<Tok>) -> bool {
        if !seen.insert(t) {
            return false;
        }
        match self.g.m.toks[t as usize] {
            TokKind::Func(f) => {
                self.g.m.funcs[f as usize].compiler_owned
                    && self.g.m.funcs[f as usize]
                        .name
                        .ends_with(":__perry_cjs_factory")
            }
            TokKind::Bound(b) => {
                let p = &self.g.m.nodes[self.g.m.bound[b as usize].0 as usize].pts;
                !p.contains_tok(TOP)
                    && p.to_sorted()
                        .into_iter()
                        .any(|t| self.factory_token(t, seen))
            }
            _ => false,
        }
    }
    fn values(&self, n: NodeId) -> Vec<Tok> {
        self.g.m.nodes[n as usize]
            .pts
            .to_sorted()
            .into_iter()
            .filter(|&t| !self.private_token(t, &mut FxHashSet::default()))
            .collect()
    }
    fn prototype(&mut self, f: FnIdx, n: NodeId) {
        let p = &self.g.m.nodes[n as usize];
        if p.prim & (prim::NULL | prim::UNDEF) != 0
            || (p.prim == 0 && p.pts.is_empty())
            || p.pts
                .to_sorted()
                .into_iter()
                .any(|t| t == TOP || matches!(self.g.m.toks[t as usize], TokKind::ObjectProto))
        {
            self.reason(
                f,
                "prototype may expose a Function constructor or global loader".into(),
            );
        }
    }
    fn function(&mut self, f: FnIdx) {
        if self.live.insert(f) {
            self.queue.push_back(f);
        }
    }
    fn module(&mut self, mi: u32) {
        if !self.modules.insert(mi) {
            return;
        }
        if let Some(f) = self.init[mi as usize] {
            self.function(f);
        }
        let module = self.g.modules[mi as usize].1;
        for import in &module.imports {
            if import.type_only
                || import.runtime_erased
                || import.is_dynamic
                || import.is_deferred_require
            {
                continue;
            }
            if let Some(path) = &import.resolved_path {
                if let Some(&m) = self.g.path_index.get(path) {
                    self.module(m);
                }
            }
            if native_capability(&import.source) {
                let internal=module.class_family_syntax.as_ref().is_some_and(|s|s.original_source_ranges.is_some()) && module.is_commonjs_wrap() && import.specifiers.iter().all(|sp| matches!(sp,perry_hir::ImportSpecifier::Named {local, ..} if local.starts_with("__perry_cjs_")));
                if !internal {
                    if let Some(f) = self.init[mi as usize] {
                        self.reason(f, format!("loader import {}", import.source));
                    }
                }
            }
        }
    }
    fn reason(&mut self, f: FnIdx, why: String) {
        if self.refusal.is_none() {
            let info = &self.g.m.funcs[f as usize];
            self.refusal = Some(format!(
                "{} in {} ({})",
                why, info.name, self.g.modules[info.module as usize].0
            ));
        }
    }
    fn node(&mut self, n: NodeId) {
        for t in self.values(n) {
            self.touch(t);
        }
    }
    fn touch(&mut self, t: Tok) {
        if self.private_token(t, &mut FxHashSet::default()) {
            return;
        }
        if t == TOP {
            self.top();
            return;
        }
        if !self.touched.insert(t) {
            return;
        }
        if self.require_token(t, &mut FxHashSet::default())
            || self.module_record(t)
            || matches!(
                self.g.m.toks[t as usize],
                TokKind::Builtin(BuiltinNs::GlobalThis | BuiltinNs::Process)
            )
        {
            self.reason(
                self.context,
                format!("{} reached by unknown/protocol code", self.g.m.tok_name(t)),
            );
            return;
        }
        if let Some(cs) = self.protocols.get(&t).cloned() {
            for c in cs {
                self.call(self.context, c, true, false);
            }
        }
        let m = &self.g.m;
        match m.toks[t as usize] {
            TokKind::Func(f) => {
                self.function(f);
                self.node(m.funcs[f as usize].ret);
            }
            TokKind::Bound(b) => {
                let (target, _, args) = &m.bound[b as usize];
                self.node(*target);
                for &n in args {
                    self.node(n);
                }
            }
            TokKind::ClassObj(tm) => self.class(tm),
            _ => {}
        }
        if let Some(&n) = m.field_any.get(&t) {
            self.node(n);
        }
        if let Some(keys) = m.field_keys.get(&t) {
            for &k in keys {
                if let Some(&n) = m.fields.get(&(t, k)) {
                    self.node(n);
                }
            }
        }
        if let Some(&n) = m.proto_node.get(&t) {
            self.node(n);
        }
        if let Some(keys) = m.accessor_keys.get(&t) {
            for &k in keys {
                if let Some((gs, ss)) = m.accessors.get(&(t, k)) {
                    for &f in gs.iter().chain(ss) {
                        self.function(f);
                    }
                }
            }
        }
        if let Some(fs) = self.extra_accessors.get(&t).cloned() {
            for f in fs {
                self.function(f);
            }
        }
    }
    fn class(&mut self, mut tm: TmplId) {
        let mut seen = FxHashSet::default();
        while seen.insert(tm) {
            if let Some(f) = self.g.m.tmpls[tm as usize].ctor {
                self.function(f);
            }
            if let Some(&f) = self.g.field_init_fn.get(&tm) {
                self.function(f);
            }
            let Some(p) = self.g.m.tmpls[tm as usize].parent else {
                break;
            };
            tm = p;
        }
    }
    fn call_target(&mut self, t: Tok, seen: &mut FxHashSet<Tok>) {
        if !seen.insert(t) {
            return;
        }
        match self.g.m.toks[t as usize] {
            TokKind::Top => self.top(),
            TokKind::Func(f) => self.function(f),
            TokKind::ClassObj(tm) => self.class(tm),
            TokKind::Bound(b) => {
                for construct in [false, true] {
                    if let Some(&c) = self.g.m.bound_merged.get(&(b, construct)) {
                        self.call(self.context, c, false, false);
                    }
                }
                let n = self.g.m.bound[b as usize].0;
                for t in self.g.m.nodes[n as usize].pts.to_sorted() {
                    self.call_target(t, seen);
                }
            }
            // Invariance facts deliberately inject the Function namespace
            // into arbitrary global/computed lookups. That summary token is
            // not a loader access path. Actual capability reads below (and
            // native/deferred operations) establish the loader boundary.
            _ => {}
        }
    }
    // TOP contains every escaped user value. Generated loader closures and
    // records are private; any source read exposing them is a Reference op.
    // Do not run a private factory merely because compiler bookkeeping
    // escaped it to the invariance solver's global summary.
    fn top(&mut self) {
        if std::mem::replace(&mut self.top_reached, true) {
            return;
        }
        let saved = std::mem::replace(&mut self.in_top_graph, true);
        let m = &self.g.m;
        for (&t, _) in &m.escaped {
            match m.toks[t as usize] {
                TokKind::Func(f) if !m.funcs[f as usize].compiler_owned => self.touch(t),
                TokKind::Obj(_) | TokKind::ClassObj(_) | TokKind::Bound(_)
                    if !self.module_record(t) =>
                {
                    self.touch(t)
                }
                _ => {}
            }
        }
        self.in_top_graph = saved;
    }
    fn call(&mut self, f: FnIdx, c: CallId, user: bool, literal: bool) {
        if !self.active_calls.insert(c) {
            return;
        }
        let mut seen = FxHashSet::default();
        for t in self.calls[c as usize].clone() {
            if user && self.private_token(t, &mut FxHashSet::default()) {
                continue;
            }
            if user && self.factory_token(t, &mut FxHashSet::default()) {
                self.reason(
                    f,
                    "private module factory called from original source".into(),
                );
            }
            if user && !literal && self.require_token(t, &mut FxHashSet::default()) {
                self.reason(
                    f,
                    "require called without a closed non-loader literal".into(),
                );
            }
            self.call_target(t, &mut seen);
        }
        for op in self.call_ops[c as usize].clone() {
            match *op {
                Op::Access(n, k, store, user) => self.access(f, n, k, store, user),
                Op::Index(n, _, user, store) => self.access(f, n, Key::Any, store, user),
                Op::Touch(n) => self.node(n),
                Op::Escape(n) => self.escape(f, n),
                Op::Token(t) => self.touch(t),
                _ => {}
            }
        }
        for child in self.children[c as usize].clone() {
            self.call(f, child, user, false);
        }
        // A known builtin's callback calls are explicit children. An opaque
        // call may invoke any callback reachable in its argument graph.
        if seen
            .iter()
            .any(|&t| matches!(self.g.m.toks[t as usize], TokKind::Top | TokKind::BuiltinFn))
        {
            let call = &self.g.m.calls[c as usize];
            for &a in call.args.iter().chain(&call.spread) {
                self.escape(f, a);
            }
            match call.this {
                ThisArg::Node(n) => self.escape(f, n),
                ThisArg::Tok(t) => self.touch(t),
                _ => {}
            }
        }
    }
    fn escape(&mut self, f: FnIdx, n: NodeId) {
        let mut todo = self.values(n);
        let mut seen = FxHashSet::default();
        while let Some(t) = todo.pop() {
            if !seen.insert(t) {
                continue;
            }
            let m = &self.g.m;
            if self.require_token(t, &mut FxHashSet::default())
                || self.module_record(t)
                || matches!(
                    m.toks[t as usize],
                    TokKind::Builtin(BuiltinNs::GlobalThis | BuiltinNs::Process | BuiltinNs::Other)
                        | TokKind::ObjectProto
                )
            {
                self.reason(f, format!("{} exposed to unknown code", m.tok_name(t)));
                return;
            }
            self.touch(t);
            if let Some(&n) = m.field_any.get(&t) {
                todo.extend(self.values(n));
            }
            if let Some(keys) = m.field_keys.get(&t) {
                for &k in keys {
                    if let Some(&n) = m.fields.get(&(t, k)) {
                        todo.extend(self.values(n));
                    }
                }
            }
            if let TokKind::Func(target) = m.toks[t as usize] {
                todo.extend(self.values(m.funcs[target as usize].ret));
            }
        }
    }
    fn require_token(&self, t: Tok, seen: &mut FxHashSet<Tok>) -> bool {
        if !seen.insert(t) {
            return false;
        }
        match self.g.m.toks[t as usize] {
            TokKind::Func(f) => {
                self.g.m.funcs[f as usize].compiler_owned
                    && self.g.m.funcs[f as usize].name.ends_with(":require")
            }
            TokKind::Bound(b) => {
                let p = &self.g.m.nodes[self.g.m.bound[b as usize].0 as usize].pts;
                !p.contains_tok(TOP)
                    && p.to_sorted()
                        .into_iter()
                        .any(|t| self.require_token(t, seen))
            }
            _ => false,
        }
    }
    fn module_record(&self, t: Tok) -> bool {
        self.g
            .m
            .tmpl_of(t)
            .is_some_and(|tm| self.g.m.reach_private_records.contains(&tm))
    }
    fn access(&mut self, f: FnIdx, n: NodeId, key: Key, store: bool, loader_names: bool) {
        let m = &self.g.m;
        let name = match key {
            Key::Named(a) => Some(m.atoms[a as usize].as_str()),
            Key::Any => None,
        };
        // The invariance model binds an unbound ordinary function receiver
        // to undefined; the runtime supplies globalThis for sloppy calls.
        // Its aliases/returns retain that primitive bit. A missing receiver
        // cannot prove a loader-name access absent, even alongside known objects.
        let receiver = &m.nodes[n as usize];
        if loader_names
            && possible_loader_member(name)
            && (receiver.prim & (prim::NULL | prim::UNDEF) != 0
                || (receiver.prim == 0 && receiver.pts.is_empty()))
        {
            self.reason(
                f,
                format!(
                    "unresolved receiver may reach {} [node {n}]",
                    name.unwrap_or("[computed]")
                ),
            );
            return;
        }
        let mut todo = self.values(n);
        let mut seen = FxHashSet::default();
        while let Some(t) = todo.pop() {
            if !seen.insert(t) {
                continue;
            }
            let private_loader = name.is_some_and(|n| {
                n.starts_with("__perry_cjs_")
                    || n.starts_with("__cjs_")
                    || (n.starts_with("__perry_")
                        && (n.contains("path_module")
                            || n.contains("cjs_main_module")
                            || n.starts_with("__perry_require_")))
            });
            let require = self.require_token(t, &mut FxHashSet::default())
                && name.is_none_or(|n| {
                    matches!(
                        n,
                        "cache" | "extensions" | "main" | "constructor" | "call" | "apply" | "bind"
                    )
                });
            let record = self.module_record(t)
                && name.is_none_or(|n| {
                    !matches!(
                        n,
                        "exports" | "id" | "path" | "filename" | "loaded" | "paths"
                    ) || (store && n != "exports")
                });
            let cap = loader_names
                && (private_loader
                    || require
                    || record
                    || (self.private_arguments.contains(&t) && name.is_none_or(|n| n == "callee"))
                    || match m.toks[t as usize] {
                        TokKind::Top => {
                            name.is_none()
                                || name.is_some_and(|n| {
                                    matches!(
                                        n,
                                        "Function"
                                            | "eval"
                                            | "module"
                                            | "Bun"
                                            | "Deno"
                                            | "cache"
                                            | "_cache"
                                            | "require"
                                            | "mainModule"
                                            | "getBuiltinModule"
                                            | "createRequire"
                                            | "binding"
                                            | "_linkedBinding"
                                            | "constructor"
                                            | "caller"
                                            | "callee"
                                            | "extensions"
                                            | "_extensions"
                                            | "_compile"
                                            | "_load"
                                    )
                                })
                        }
                        TokKind::Builtin(BuiltinNs::GlobalThis | BuiltinNs::Other) => {
                            name.is_none()
                                || name.is_some_and(|n| {
                                    matches!(
                                        n,
                                        "require"
                                            | "module"
                                            | "Function"
                                            | "eval"
                                            | "Bun"
                                            | "Deno"
                                            | "dlopen"
                                            | "createRequire"
                                    )
                                })
                        }
                        TokKind::Builtin(BuiltinNs::Process) => {
                            name.is_none()
                                || name.is_some_and(|n| {
                                    matches!(
                                        n,
                                        "mainModule"
                                            | "getBuiltinModule"
                                            | "binding"
                                            | "_linkedBinding"
                                            | "dlopen"
                                    )
                                })
                        }
                        TokKind::Func(_)
                        | TokKind::Bound(_)
                        | TokKind::ClassObj(_)
                        | TokKind::BuiltinFn => {
                            name.is_none() || matches!(name, Some("constructor" | "caller"))
                        }
                        TokKind::Builtin(_) => name == Some("constructor"),
                        TokKind::ObjectProto if m.nodes[n as usize].pts.contains_tok(t) => {
                            name.is_none() || name == Some("constructor")
                        }
                        _ => false,
                    });
            if cap {
                self.reason(
                    f,
                    format!(
                        "reachable {}.{}{} [node {n}]",
                        m.tok_name(t),
                        name.unwrap_or("[computed]"),
                        if store { " write" } else { " read" }
                    ),
                );
                return;
            }
            if let Some(fs) = self.extra_accessors.get(&t).cloned() {
                for f in fs {
                    self.function(f);
                    self.node(m.funcs[f as usize].ret);
                }
            }
            if t == TOP {
                for (gs, ss) in m.accessors.values() {
                    for &f in gs.iter().chain(ss) {
                        self.function(f);
                    }
                }
                for fs in self.extra_accessors.values().cloned().collect::<Vec<_>>() {
                    for f in fs {
                        self.function(f);
                    }
                }
            }
            if let Some(&p) = m.proto_node.get(&t) {
                todo.extend(m.nodes[p as usize].pts.to_sorted());
            }
            let keys: Vec<_> = match key {
                Key::Named(a) => vec![a],
                Key::Any => m.accessor_keys.get(&t).cloned().unwrap_or_default(),
            };
            for k in keys {
                if let Some((get, set)) = m.accessors.get(&(t, k)) {
                    for &f in if store { set } else { get } {
                        self.function(f);
                    }
                }
            }
        }
    }
}

pub(crate) fn refusal<'a>(g: &'a Gen<'a>, entry: Option<&str>) -> Option<String> {
    if g.m.aborted {
        return Some("whole-program solve aborted".into());
    }
    let mut r = Reach {
        g,
        ops: vec![Vec::new(); g.m.funcs.len()],
        calls: vec![Vec::new(); g.m.calls.len()],
        live: Default::default(),
        queue: Default::default(),
        touched: Default::default(),
        modules: Default::default(),
        init: vec![None; g.modules.len()],
        extra_accessors: Default::default(),
        refusal: None,
        context: 0,
        children: vec![Vec::new(); g.m.calls.len()],
        active_calls: Default::default(),
        call_ops: vec![Vec::new(); g.m.calls.len()],
        protocols: Default::default(),
        top_reached: false,
        in_top_graph: false,
        private_arguments: Default::default(),
    };
    for f in &g.m.funcs {
        if f.compiler_owned && !f.compiler_body && f.has_arguments {
            if let Some(n) = f.rest {
                r.private_arguments
                    .extend(g.m.nodes[n as usize].pts.to_sorted());
            }
        }
    }
    for (c, op) in &g.m.reach_call_ops {
        r.call_ops[*c as usize].push(op);
    }
    for (c, site) in g.m.calls.iter().enumerate() {
        if let Some(parent) = site.reach_parent {
            if parent != c as u32 {
                r.children[parent as usize].push(c as u32);
            }
        }
        if g.m.protocol_calls.contains(&(c as u32)) {
            if let ThisArg::Tok(t) = site.this {
                r.protocols.entry(t).or_default().push(c as u32);
            }
        }
    }
    // Computed object accessors use the solver's STAR-key rule, which
    // escapes their function rather than installing a named accessor. Its
    // retained constraint still identifies both the holder and the body.
    let star = g.m.atom_index.get(crate::STAR_KEY).copied();
    for node in &g.m.nodes {
        for cons in &node.cons {
            if let Cons::AccessorInstall { key, f, .. } = *cons {
                if Some(key) == star {
                    for t in node.pts.to_sorted() {
                        r.extra_accessors.entry(t).or_default().push(f);
                    }
                }
            }
        }
    }
    for &(c, t, _) in &g.m.bind_done {
        r.calls[c as usize].push(t);
    }
    for &(c, t) in &g.m.construct_done {
        r.calls[c as usize].push(t);
    }
    for (f, op) in &g.m.reach_ops {
        r.ops[*f as usize].push(op);
        if let Op::Accessor(t, target) = op {
            r.extra_accessors.entry(*t).or_default().push(*target);
        }
        if matches!(op, Op::Root) {
            r.init[g.m.funcs[*f as usize].module as usize] = Some(*f);
        }
    }
    match entry {
        Some(path) => match g.path_index.get(path) {
            Some(&mi) => r.module(mi),
            None => return Some(format!("entry module missing from solved graph: {path}")),
        },
        None => {
            for mi in 0..g.modules.len() {
                r.module(mi as u32);
            }
        }
    }
    // The driver already computes eager static dependencies, including
    // re-exports, cycles and package identity. Deferred modules are seeded
    // only when a reachable import/require operation reaches them.
    if entry.is_some() {
        for (mi, (_, module)) in g.modules.iter().enumerate() {
            if module.init_kind == perry_hir::ModuleInitKind::Eager {
                r.module(mi as u32);
            }
        }
    }
    while let Some(f) = r.queue.pop_front() {
        r.context = f;
        for op in r.ops[f as usize].clone() {
            match *op {
                Op::Root | Op::Accessor(..) => {}
                Op::Reference(n, kind) => {
                    let cap = kind == 3
                        || g.m.nodes[n as usize].pts.to_sorted().into_iter().any(|t| {
                            t == TOP
                                || (kind == 1 && r.require_token(t, &mut FxHashSet::default()))
                                || (kind == 2 && r.module_record(t))
                        });
                    if cap {
                        r.reason(f, format!("source reference to loader capability ({kind})"));
                    }
                }
                Op::Value(n, allow_require) => {
                    if g.m.nodes[n as usize].pts.contains_tok(TOP) {
                        continue;
                    }
                    for t in g.m.nodes[n as usize].pts.to_sorted() {
                        if let TokKind::Func(target) = g.m.toks[t as usize] {
                            let info = &g.m.funcs[target as usize];
                            if info.compiler_owned
                                && !(allow_require && info.name.ends_with(":require"))
                            {
                                r.reason(
                                    f,
                                    format!(
                                        "source reference to private loader function {}",
                                        info.name
                                    ),
                                );
                            }
                        }
                    }
                }
                Op::Module(mi) => r.module(mi),
                Op::Function(target) => r.function(target),
                Op::Prototype(n) => r.prototype(f, n),
                Op::LoaderCall(n, ref literal) => {
                    for t in g.m.nodes[n as usize].pts.to_sorted() {
                        if r.require_token(t, &mut FxHashSet::default()) {
                            r.reason(
                                f,
                                format!(
                                    "require may reach {}",
                                    literal.as_deref().unwrap_or("a non-constant module")
                                ),
                            );
                        }
                    }
                }
                Op::Call(c, user, literal) => r.call(f, c, user, literal),
                Op::Touch(n) => r.node(n),
                Op::Token(t) => r.touch(t),
                Op::Escape(n) => r.escape(f, n),
                Op::Access(n, k, store, user) => r.access(f, n, k, store, user),
                Op::Index(n, idx, user, store) => {
                    let key = &g.m.nodes[idx as usize];
                    let names = user
                        && (key.prim & !(prim::NUM | prim::SYMBOL) != 0 || !key.pts.is_empty());
                    r.access(f, n, Key::Any, store, names);
                }
                Op::Capability(ref s) => r.reason(f, s.clone()),
            }
            if r.refusal.is_some() {
                return r.refusal;
            }
        }
    }
    None
}
