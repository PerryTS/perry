//! Default-off, compiler-only semantic reachability census. This pass never
//! filters HIR, registrations, LLVM, or metadata. Function value existence
//! conservatively implies executable-body reachability. Consequently callbacks,
//! bound functions, object fields, reflection and interpreter arguments need no
//! special call-target narrowing: every value they can receive is already live.
//!
//! Class values retain the complete class (including ancestors and accessors).
//! Dynamic namespaces retain all exports. These conservative edges are labelled
//! and solved a second time without them to expose precision costs, not savings.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Mutex;

use anyhow::{bail, Result};
use perry_hir::{Class, Expr, Function, Module, Stmt};

use super::helpers::{sanitize, scoped_fn_name, scoped_method_name, scoped_static_method_name};

#[derive(Debug, Clone)]
struct Edge {
    to: usize,
    reason: &'static str,
    coarse: bool,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub module: String,
    pub label: String,
    pub symbol: Option<String>,
    edges: Vec<Edge>,
}

#[derive(Debug, Clone)]
pub struct Verdict {
    pub reachable: bool,
    pub only_conservative: bool,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct MachineFunction {
    pub symbol: String,
    pub module: String,
    pub node: Option<usize>,
    pub references: Vec<String>,
    pub precise_references: Vec<String>,
}

#[derive(Debug)]
pub struct Census {
    pub nodes: Vec<Node>,
    pub verdicts: Vec<Verdict>,
    bases: BTreeMap<String, usize>,
    module_paths: BTreeMap<String, String>,
    module_inits: BTreeMap<String, usize>,
    machines: Mutex<Vec<MachineFunction>>,
    global_refs: Mutex<BTreeMap<String, Vec<String>>>,
    precise: Vec<bool>,
    namespace_nodes: BTreeMap<String, usize>,
    namespace_getters: Mutex<BTreeMap<String, usize>>,
    export_nodes: BTreeMap<(String, String), usize>,
}

struct Body<'a> {
    node: usize,
    module: usize,
    stmts: &'a [Stmt],
    defaults: Vec<&'a Expr>,
}

struct Build<'a> {
    modules: &'a [(String, &'a Module)],
    links: &'a BTreeMap<(String, String), String>,
    paths: HashMap<String, usize>,
    nodes: Vec<Node>,
    bases: BTreeMap<String, usize>,
    funcs: HashMap<(usize, u32), usize>,
    names: HashMap<(usize, String), usize>,
    classes: HashMap<(usize, String), usize>,
    exports: HashMap<(usize, String), usize>,
    export_names: HashMap<String, Vec<usize>>,
    module_exports: Vec<Vec<usize>>,
    inits: Vec<usize>,
    namespaces: Vec<usize>,
    bodies: Vec<Body<'a>>,
    roots: Vec<(usize, &'static str, bool)>,
    seen_closures: HashSet<usize>,
    decorated: Vec<(usize, usize, &'a perry_hir::Decorator)>,
}

impl<'a> Build<'a> {
    fn node(&mut self, mi: usize, label: String, symbol: Option<String>) -> usize {
        let n = self.nodes.len();
        if let Some(s) = &symbol {
            self.bases.insert(s.clone(), n);
        }
        self.nodes.push(Node {
            module: self.modules[mi].0.clone(),
            label,
            symbol,
            edges: vec![],
        });
        n
    }

    fn edge(&mut self, from: usize, to: usize, reason: &'static str, coarse: bool) {
        self.nodes[from].edges.push(Edge { to, reason, coarse });
    }

    fn target(&self, mi: usize, source: &str) -> Option<usize> {
        let path = self
            .links
            .get(&(self.modules[mi].0.clone(), source.to_string()));
        path.and_then(|p| self.paths.get(p))
            .copied()
            .or_else(|| self.paths.get(source).copied())
    }

    fn function(&mut self, mi: usize, f: &'a Function, symbol: String) -> usize {
        let n = if let Some(&n) = self.funcs.get(&(mi, f.id)) {
            // Monomorphized class members can share a source FuncId. Keep
            // their complete lowering family and the union of actual bodies.
            self.bases.insert(symbol, n);
            n
        } else {
            let n = self.node(mi, f.name.clone(), Some(symbol));
            self.funcs.insert((mi, f.id), n);
            n
        };
        for d in &f.decorators {
            self.decorated.push((mi, n, d));
        }
        self.bodies.push(Body {
            node: n,
            module: mi,
            stmts: &f.body,
            defaults: f
                .params
                .iter()
                .filter_map(|p| p.default.as_ref())
                .chain(f.decorators.iter().flat_map(|d| d.args.iter()))
                .collect(),
        });
        n
    }

    fn closures(&mut self, mi: usize, expr: &'a Expr) {
        if let Expr::Closure {
            func_id,
            body,
            params,
            ..
        } = expr
        {
            if self.seen_closures.insert(expr as *const Expr as usize) {
                let symbol = format!(
                    "perry_closure_{}__{}",
                    sanitize(&self.modules[mi].1.name),
                    func_id
                );
                let n = if let Some(&n) = self.funcs.get(&(mi, *func_id)) {
                    n
                } else {
                    let n = self.node(mi, format!("closure#{func_id}"), Some(symbol.clone()));
                    self.funcs.insert((mi, *func_id), n);
                    n
                };
                self.bases.insert(symbol, n);
                self.bodies.push(Body {
                    node: n,
                    module: mi,
                    stmts: body,
                    defaults: params.iter().filter_map(|p| p.default.as_ref()).collect(),
                });
            }
        }
        perry_hir::walker::walk_expr_children(expr, &mut |e| self.closures(mi, e));
    }

    fn class(&mut self, mi: usize, c: &'a Class) {
        let p = sanitize(&self.modules[mi].1.name);
        let n = self.node(mi, format!("class:{}", c.name), None);
        self.classes.insert((mi, c.name.clone()), n);
        self.names.insert((mi, c.name.clone()), n);
        for a in &c.aliases {
            self.classes.insert((mi, a.clone()), n);
        }
        // Synthesized constructors also have machine code and share the class's
        // liveness even when the HIR has no explicit constructor body.
        let ctor = format!("{}__{}_constructor", p, c.name);
        self.bases.insert(ctor.clone(), n);
        if let Some(f) = &c.constructor {
            let f = self.function(mi, f, ctor);
            self.edge(n, f, "class-constructor", false);
        }
        for f in &c.methods {
            let f = self.function(mi, f, scoped_method_name(&p, &c.name, &f.name));
            self.edge(n, f, "whole-class-members", true);
        }
        for f in &c.static_methods {
            let f = self.function(mi, f, scoped_static_method_name(&p, c.id, &c.name, &f.name));
            self.edge(n, f, "whole-class-members", true);
        }
        for (kind, fs) in [("__get_", &c.getters), ("__set_", &c.setters)] {
            for (_, f) in fs {
                let name = format!("{kind}{}", f.name);
                let symbol = if c.static_accessor_fn_ids.contains(&f.id) {
                    scoped_static_method_name(&p, c.id, &c.name, &name)
                } else {
                    scoped_method_name(&p, &c.name, &name)
                };
                let f = self.function(mi, f, symbol);
                self.edge(n, f, "whole-class-accessors", true);
            }
        }
        for member in &c.computed_members {
            let f = &member.function;
            let symbol = if member.is_static {
                scoped_static_method_name(&p, c.id, &c.name, &f.name)
            } else {
                scoped_method_name(&p, &c.name, &f.name)
            };
            let f = self.function(mi, f, symbol);
            self.edge(n, f, "whole-class-computed-members", true);
        }
    }

    fn exported(&mut self, mi: usize, name: &str) -> usize {
        if let Some(&n) = self.exports.get(&(mi, name.to_owned())) {
            return n;
        }
        let n = self.node(mi, format!("export:{name}"), None);
        self.exports.insert((mi, name.to_owned()), n);
        self.export_names
            .entry(name.to_owned())
            .or_default()
            .push(n);
        self.module_exports[mi].push(n);
        n
    }

    fn namespace(&mut self, from: usize, mi: usize, reason: &'static str) {
        self.edge(from, self.inits[mi], reason, false);
        self.edge(from, self.namespaces[mi], reason, false);
        for n in self.module_exports[mi].clone() {
            self.edge(self.namespaces[mi], n, "namespace-binding", true);
        }
    }

    fn named(&mut self, from: usize, mi: usize, name: &str, reason: &'static str) {
        if let Some(&n) = self
            .names
            .get(&(mi, name.to_owned()))
            .or_else(|| self.classes.get(&(mi, name.to_owned())))
        {
            self.edge(from, n, reason, false);
            return;
        }
        if let Some((class, _)) = name.split_once('.') {
            if let Some(&n) = self.classes.get(&(mi, class.to_owned())) {
                self.edge(from, n, reason, false);
                return;
            }
        }
        // Imported names, including namespace-qualified members. Whole-namespace
        // retention is deliberate when the compiler's lowered reference cannot
        // preserve the binding's original provenance.
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
                        (local, Some(imported.as_str()))
                    }
                    perry_hir::ImportSpecifier::Default { local } => (local, Some("default")),
                    perry_hir::ImportSpecifier::Namespace { local } => (local, None),
                };
                if name == local {
                    if let Some(exported) = exported {
                        let n = self.exported(target, exported);
                        self.edge(from, n, reason, false);
                    } else {
                        self.namespace(from, target, "namespace-observation");
                    }
                    return;
                }
                if name.strip_prefix(local).is_some_and(|s| s.starts_with('.')) {
                    let member = &name[local.len() + 1..];
                    let n = self.exported(target, member);
                    self.edge(from, n, reason, false);
                    return;
                }
            }
        }
        // Native runtime names do not designate compiled JS functions. For
        // flattened extern names lacking an alias, retain matching exports.
        let candidates = self.export_names.get(name).cloned().unwrap_or_default();
        for n in candidates {
            self.edge(from, n, "unresolved-extern-name", true);
        }
    }

    fn expr(&mut self, mi: usize, from: usize, expr: &Expr) {
        self.expr_kind(mi, from, expr, false);
    }

    fn expr_kind(&mut self, mi: usize, from: usize, expr: &Expr, eval_scope: bool) {
        if let Expr::Call { callee, args, .. } = expr {
            if matches!(callee.as_ref(), Expr::ExternFuncRef { name, .. } if name == "js_eval_in_scope")
            {
                self.expr(mi, from, callee);
                for (i, arg) in args.iter().enumerate() {
                    self.expr_kind(mi, from, arg, i == 1 || i == 2);
                }
                return;
            }
        }
        match expr {
            Expr::FuncRef(fid) => {
                if let Some(&n) = self.funcs.get(&(mi, *fid)) {
                    self.edge(from, n, "function-value", false);
                }
            }
            Expr::Closure { func_id, .. } => {
                if let Some(&n) = self.funcs.get(&(mi, *func_id)) {
                    self.edge(
                        from,
                        n,
                        if eval_scope {
                            "eval-lexical-scope"
                        } else {
                            "closure-value/callback"
                        },
                        eval_scope,
                    );
                }
                // Defaults and body execute only when the callable is activated.
                return;
            }
            Expr::ExternFuncRef { name, .. } => self.named(from, mi, name, "live-import"),
            Expr::ClassRef(name)
            | Expr::New {
                class_name: name, ..
            }
            | Expr::StaticFieldGet {
                class_name: name, ..
            }
            | Expr::StaticFieldSet {
                class_name: name, ..
            }
            | Expr::ClassMethodHome {
                class_name: name, ..
            }
            | Expr::RegisterClassComputedMethod {
                class_name: name, ..
            }
            | Expr::RegisterClassComputedAccessor {
                class_name: name, ..
            }
            | Expr::ClassExprFresh { template: name, .. } => {
                self.named(from, mi, name, "class-value")
            }
            Expr::DynamicImport {
                paths,
                deferred_error,
                ..
            } => {
                if deferred_error.is_none() {
                    if paths.is_empty() {
                        for target in 0..self.modules.len() {
                            self.namespace(from, target, "unknown-import-eligible-set");
                        }
                    }
                    for path in paths {
                        if let Some(target) = self.target(mi, path) {
                            self.namespace(from, target, "dynamic-import-namespace");
                        } else {
                            for target in 0..self.modules.len() {
                                self.namespace(from, target, "unknown-import-eligible-set");
                            }
                        }
                    }
                }
            }
            Expr::WorkerNew {
                paths,
                partial,
                is_eval,
                ..
            } => {
                if *partial && !*is_eval {
                    for target in 0..self.modules.len() {
                        self.namespace(from, target, "unknown-worker-eligible-set");
                    }
                }
                for path in paths {
                    if let Some(target) = self.target(mi, path) {
                        self.edge(from, self.inits[target], "worker-entry", false);
                    } else {
                        for target in 0..self.modules.len() {
                            self.namespace(from, target, "unknown-worker-eligible-set");
                        }
                    }
                }
            }
            _ => {}
        }
        // Exhaustive central walker covers callback arrays, objects, protocols,
        // returns, throws, bind/apply and interpreter scope-accessor operands.
        perry_hir::walker::walk_expr_children(expr, &mut |e| {
            self.expr_kind(mi, from, e, eval_scope)
        });
    }
}

impl Census {
    pub fn compute(
        modules: &[(String, &Module)],
        links: &BTreeMap<(String, String), String>,
        entry: &str,
        library: bool,
        dynamic: &perry_hir::dynamic_code::DynamicCodeReach,
    ) -> Self {
        let mut b = Build {
            modules,
            links,
            paths: modules
                .iter()
                .enumerate()
                .map(|(i, (p, _))| (p.clone(), i))
                .collect(),
            nodes: vec![],
            bases: BTreeMap::new(),
            funcs: HashMap::new(),
            names: HashMap::new(),
            classes: HashMap::new(),
            exports: HashMap::new(),
            export_names: HashMap::new(),
            module_exports: vec![vec![]; modules.len()],
            inits: vec![],
            namespaces: vec![],
            bodies: vec![],
            roots: vec![],
            seen_closures: HashSet::new(),
            decorated: vec![],
        };
        for (mi, (_, m)) in modules.iter().enumerate() {
            let init = b.node(mi, "module-evaluation".into(), None);
            b.inits.push(init);
            let namespace = b.node(mi, "namespace-observation".into(), None);
            b.namespaces.push(namespace);
            let prefix = sanitize(&m.name);
            b.bases
                .insert(format!("__perry_namespace_observed_{prefix}"), namespace);
            for suffix in ["__init", "__init_body", "__init_evaluate"] {
                b.bases.insert(format!("{prefix}{suffix}"), init);
            }
            if modules[mi].0 == entry {
                b.bases.insert("main".into(), init);
                b.bases.insert("perry_module_init".into(), init);
            }
            b.bodies.push(Body {
                node: init,
                module: mi,
                stmts: &m.init,
                defaults: m.globals.iter().filter_map(|g| g.init.as_ref()).collect(),
            });
            for f in &m.functions {
                let n = b.function(mi, f, scoped_fn_name(&sanitize(&m.name), &f.name));
                b.names.insert((mi, f.name.clone()), n);
                // Module init registers the lifted well-known-symbol class hooks
                // (see codegen static_fields), so evaluation of the module owns them.
                if f.name.starts_with("__perry_wk_hasinstance_")
                    || f.name.starts_with("__perry_wk_tostringtag_")
                {
                    b.edge(init, n, "class-wellknown-hook", false);
                }
            }
            for c in &m.classes {
                b.class(mi, c);
            }
            for c in &m.classes {
                for field in c.fields.iter().chain(&c.static_fields) {
                    if let Some(e) = &field.init {
                        b.closures(mi, e);
                    }
                    if let Some(e) = &field.key_expr {
                        b.closures(mi, e);
                    }
                }
                if let Some(e) = &c.extends_expr {
                    b.closures(mi, e);
                }
                for d in &c.decorators {
                    for e in &d.args {
                        b.closures(mi, e);
                    }
                }
            }
            for (name, _) in &m.exported_functions {
                b.exported(mi, name);
            }
            for e in &m.exports {
                match e {
                    perry_hir::Export::Named { exported, .. }
                    | perry_hir::Export::ReExport { exported, .. } => {
                        b.exported(mi, exported);
                    }
                    perry_hir::Export::NamespaceReExport { name, .. } => {
                        b.exported(mi, name);
                    }
                    _ => {}
                }
            }
        }
        // Resolve the visible names of star barrels before namespace roots.
        // Static export declarations themselves do not observe callable values.
        loop {
            let mut changed = false;
            for (mi, (_, m)) in modules.iter().enumerate() {
                for e in &m.exports {
                    if let perry_hir::Export::ExportAll { source } = e {
                        if let Some(target) = b.target(mi, source) {
                            let names: Vec<_> = b.module_exports[target]
                                .iter()
                                .map(|&n| b.nodes[n].label[7..].to_owned())
                                .filter(|n| n != "default")
                                .collect();
                            for name in names {
                                if !b.exports.contains_key(&(mi, name.clone())) {
                                    b.exported(mi, &name);
                                    changed = true;
                                }
                            }
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        for (mi, n, d) in std::mem::take(&mut b.decorated) {
            b.named(n, mi, &d.name, "function-decorator");
        }
        let mut i = 0;
        while i < b.bodies.len() {
            let body = &b.bodies[i];
            let (mi, stmts, defaults) = (body.module, body.stmts, body.defaults.clone());
            for s in stmts {
                perry_hir::walker::stmt_any_expr(s, &mut |e| {
                    b.closures(mi, e);
                    false
                });
            }
            for e in defaults {
                b.closures(mi, e);
            }
            i += 1;
        }
        // Export binding constraints are conditional. Merely declaring an ESM
        // export in a closed executable never seeds it as a callable entry.
        for (mi, (path, m)) in modules.iter().enumerate() {
            if path == entry {
                b.roots.push((b.inits[mi], "entry", false));
            }
            // Match the driver's current eager-evaluation contract exactly.
            if m.init_kind == perry_hir::ModuleInitKind::Eager {
                b.roots
                    .push((b.inits[mi], "module-top-level-effects", false));
            }
            for import in &m.imports {
                if import.type_only
                    || import.runtime_erased
                    || import.is_dynamic
                    || import.is_deferred_require
                {
                    continue;
                }
                if let Some(target) = b.target(mi, &import.source) {
                    b.edge(
                        b.inits[mi],
                        b.inits[target],
                        "static-import-evaluation",
                        false,
                    );
                }
            }
            for e in &m.exports {
                if let perry_hir::Export::ReExport { source, .. }
                | perry_hir::Export::NamespaceReExport { source, .. }
                | perry_hir::Export::ExportAll { source } = e
                {
                    if let Some(target) = b.target(mi, source) {
                        b.edge(b.inits[mi], b.inits[target], "re-export-evaluation", false);
                    }
                }
                match e {
                    perry_hir::Export::Named { local, exported } => {
                        let n = b.exported(mi, exported);
                        b.named(n, mi, local, "observed-export");
                    }
                    perry_hir::Export::ReExport {
                        source,
                        imported,
                        exported,
                    } => {
                        if let Some(target) = b.target(mi, source) {
                            let n = b.exported(mi, exported);
                            let t = b.exported(target, imported);
                            b.edge(n, t, "re-export", false);
                        }
                    }
                    perry_hir::Export::NamespaceReExport { source, name } => {
                        if let Some(target) = b.target(mi, source) {
                            let n = b.exported(mi, name);
                            b.namespace(n, target, "namespace-re-export");
                        }
                    }
                    perry_hir::Export::ExportAll { source } => {
                        if let Some(target) = b.target(mi, source) {
                            let bindings: Vec<_> = b
                                .exports
                                .iter()
                                .filter(|((m, name), _)| *m == mi && name != "default")
                                .map(|((_, name), &n)| (name.clone(), n))
                                .collect();
                            for (name, n) in bindings {
                                let explicit = m.exports.iter().any(|e| match e {
                                    perry_hir::Export::Named { exported, .. }
                                    | perry_hir::Export::ReExport { exported, .. } => {
                                        exported == &name
                                    }
                                    perry_hir::Export::NamespaceReExport {
                                        name: exported, ..
                                    } => exported == &name,
                                    _ => false,
                                });
                                if !explicit {
                                    if let Some(&t) = b.exports.get(&(target, name)) {
                                        b.edge(n, t, "observed-star-binding", false);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if library && path == entry {
                for n in b
                    .exports
                    .iter()
                    .filter_map(|((m, _), &n)| (*m == mi).then_some(n))
                    .collect::<Vec<_>>()
                {
                    b.roots.push((n, "external-library-export", false));
                }
            }
            for (_, fid) in &m.script_global_functions {
                if let Some(&n) = b.funcs.get(&(mi, *fid)) {
                    b.roots.push((n, "global-publication", false));
                }
            }
            // Decision 150: all globals escape; private lexical declarations do
            // not magically escape. Direct-eval lowering explicitly materializes
            // accessors for its enclosing bindings; normal value edges below
            // include those getters, their returned functions and object fields.
            if dynamic.globals {
                for g in &m.globals {
                    if let Some(e) = &g.init {
                        b.expr(mi, b.inits[mi], e);
                    }
                }
            }
            for c in &m.classes {
                let n = b.classes[&(mi, c.name.clone())];
                if let Some(parent) = &c.extends_name {
                    b.named(n, mi, parent, "class-heritage");
                }
                if let Some(e) = &c.extends_expr {
                    b.expr(mi, n, e);
                }
                for f in c.fields.iter().chain(&c.static_fields) {
                    if let Some(e) = &f.init {
                        b.expr(mi, n, e);
                    }
                    if let Some(e) = &f.key_expr {
                        b.expr(mi, n, e);
                    }
                }
                for d in &c.decorators {
                    b.named(n, mi, &d.name, "class-decorator");
                    for e in &d.args {
                        b.expr(mi, n, e);
                    }
                }
                if !c.is_nested
                    && (!c.static_fields.is_empty()
                        || !c.computed_members.is_empty()
                        || c.extends_expr.is_some()
                        || c.extends_name.is_some()
                        || c.fields
                            .iter()
                            .any(|f| f.key_expr.is_some() || !f.decorators.is_empty())
                        || c.methods
                            .iter()
                            .chain(&c.static_methods)
                            .any(|f| !f.decorators.is_empty())
                        || c.getters
                            .iter()
                            .chain(&c.setters)
                            .any(|(_, f)| !f.decorators.is_empty())
                        || !c.decorators.is_empty())
                {
                    b.edge(b.inits[mi], n, "effectful-class-whole-fallback", true);
                }
            }
        }
        for body in std::mem::take(&mut b.bodies) {
            for s in body.stmts {
                perry_hir::walker::stmt_any_expr(s, &mut |e| {
                    b.expr(body.module, body.node, e);
                    false
                });
            }
            for e in body.defaults {
                b.expr(body.module, body.node, e);
            }
        }
        // Export spellings are ABI members of their underlying callable or
        // global getter family, not independent semantic roots. A raw local
        // getter can forward to a public spelling even when that public export
        // itself is never observed (#460 / module_globals_emit).
        for (mi, (_, m)) in modules.iter().enumerate() {
            let prefix = sanitize(&m.name);
            let mut getters = HashMap::new();
            for (exported, fid) in &m.exported_functions {
                if let Some(&body) = b.funcs.get(&(mi, *fid)) {
                    let binding = b.exported(mi, exported);
                    b.edge(binding, body, "exported-function-family", false);
                    b.bases.insert(scoped_fn_name(&prefix, exported), body);
                }
            }
            for e in &m.exports {
                if let perry_hir::Export::Named { local, exported } = e {
                    let binding = b.exported(mi, exported);
                    let symbol = scoped_fn_name(&prefix, exported);
                    let function = m
                        .exported_functions
                        .iter()
                        .find(|(name, _)| name == exported)
                        .and_then(|(_, id)| b.funcs.get(&(mi, *id)).copied())
                        .or_else(|| {
                            m.functions
                                .iter()
                                .find(|f| &f.name == local)
                                .and_then(|f| b.funcs.get(&(mi, f.id)).copied())
                        });
                    if let Some(body) = function {
                        b.bases.insert(symbol, body);
                        b.edge(binding, body, "exported-function-family", false);
                    } else {
                        let getter = *getters.entry(local.clone()).or_insert_with(|| {
                            b.node(mi, format!("binding-accessor:{local}"), None)
                        });
                        b.edge(binding, getter, "observed-binding-accessor", false);
                        b.bases.entry(symbol).or_insert(getter);
                        b.bases
                            .entry(scoped_fn_name(&prefix, local))
                            .or_insert(getter);
                    }
                }
            }
        }
        for ((mi, name), &node) in &b.exports {
            let prefix = sanitize(&modules[*mi].1.name);
            b.bases.entry(scoped_fn_name(&prefix, name)).or_insert(node);
        }
        for n in &mut b.nodes {
            n.edges.sort_by_key(|e| (e.to, e.coarse, e.reason));
            n.edges.dedup_by_key(|e| (e.to, e.coarse, e.reason));
        }
        b.roots.sort();
        b.roots.dedup();
        let full = solve(&b.nodes, &b.roots, true);
        let precise = solve(&b.nodes, &b.roots, false);
        let verdicts = full
            .into_iter()
            .zip(precise.iter())
            .map(|(reason, strict)| Verdict {
                reachable: reason.is_some(),
                only_conservative: reason.is_some() && strict.is_none(),
                reason: reason.unwrap_or_else(|| "no-semantic-root".into()),
            })
            .collect();
        Self {
            nodes: b.nodes,
            verdicts,
            bases: b.bases,
            module_inits: modules
                .iter()
                .enumerate()
                .map(|(mi, (_, m))| (sanitize(&m.name), b.inits[mi]))
                .collect(),
            module_paths: modules
                .iter()
                .map(|(p, m)| (m.name.clone(), p.clone()))
                .collect(),
            machines: Mutex::new(vec![]),
            global_refs: Mutex::new(BTreeMap::new()),
            namespace_nodes: modules
                .iter()
                .enumerate()
                .map(|(mi, (_, m))| (sanitize(&m.name), b.namespaces[mi]))
                .collect(),
            namespace_getters: Mutex::new(BTreeMap::new()),
            export_nodes: b
                .exports
                .iter()
                .map(|((mi, name), &n)| ((modules[*mi].1.name.clone(), name.clone()), n))
                .collect(),
            precise: precise.iter().map(Option::is_some).collect(),
        }
    }

    pub fn node_for_symbol(&self, symbol: &str) -> Option<usize> {
        let getter = symbol.split('$').next().unwrap_or(symbol);
        if let Some(&n) = self.namespace_getters.lock().unwrap().get(getter) {
            return Some(n);
        }
        let mut s = symbol.strip_prefix("__perry_wrap_").unwrap_or(symbol);
        if let Some(&n) = self.bases.get(s) {
            return Some(n);
        }
        if let Some((base, _)) = s.split_once('$') {
            s = base;
        }
        if let Some((base, _)) = s.split_once(".__") {
            s = base;
        }
        if let Some(base) = s.strip_suffix("__eclo") {
            s = base;
        }
        self.bases.get(s).copied().or_else(|| {
            // Codegen can outline module/CJS initialization after the final-HIR
            // census. Its new chunks retain their original evaluation owner.
            let (prefix, _) = s
                .strip_prefix("perry_fn_")?
                .split_once("____perry_entry_chunk_")?;
            self.module_inits.get(prefix).copied()
        })
    }

    /// The tiny backend proves the original syntax has no user functions,
    /// imports or dynamic operations, and emits one semantic entry. Inspect
    /// its exact complete LLVM image (including declarations/constants) rather
    /// than changing its backend selection just to obtain a report. Any future
    /// generated reference to a known dead body still trips the root contract.
    pub fn capture_tiny_ir(&self, ir: &str) -> Result<()> {
        let Some(node) = self.node_for_symbol("main") else {
            bail!("tiny reach census has no semantic entry");
        };
        let lines: Vec<_> = ir.lines().map(str::to_owned).collect();
        let references = executable_references(&lines);
        self.machines.lock().unwrap().push(MachineFunction {
            symbol: "main".into(),
            module: self.nodes[node].module.clone(),
            node: Some(node),
            precise_references: references.clone(),
            references,
        });
        Ok(())
    }

    /// Inspect the exact finalized generated function bodies, before LLVM/unit
    /// splitting. Pure pointer definitions are followed from executable sinks;
    /// addresses used exclusively by generated registration calls are excluded.
    /// Thus the existing circular class/name/info retention is not re-seeded.
    pub(crate) fn capture(&self, module: &str, llmod: &crate::module::LlModule) -> Result<()> {
        let spans = NS_SPANS.with(|s| s.borrow().clone().unwrap_or_default());
        let mut found = vec![];
        for f in llmod.diagnostic_functions() {
            let mut metadata = HashMap::new();
            for span in spans.iter().filter(|s| s.function == f.name) {
                if let Some(&owner) = self.namespace_nodes.get(&span.module) {
                    if let Some(block) = f.blocks().iter().find(|b| b.label == span.block) {
                        for inst in &block.insts()[span.start..span.end] {
                            let mut text = String::new();
                            inst.render_into(&mut text);
                            for line in text.lines() {
                                metadata.insert(line.trim().to_owned(), owner);
                            }
                        }
                    }
                }
            }
            let mut lines = vec![];
            let mut precise_lines = vec![];
            f.for_each_final_line::<std::convert::Infallible>(&mut |line| {
                if let Some(&owner) = metadata.get(line.trim()) {
                    if self.verdicts[owner].reachable {
                        lines.push(line.to_owned());
                    }
                } else {
                    lines.push(line.to_owned());
                    precise_lines.push(line.to_owned());
                }
                Ok(())
            })
            .unwrap();
            found.push(MachineFunction {
                symbol: f.name.clone(),
                module: self
                    .module_paths
                    .get(module)
                    .cloned()
                    .unwrap_or_else(|| module.into()),
                node: self.node_for_symbol(&f.name),
                references: executable_references(&lines),
                precise_references: executable_references(&precise_lines),
            });
        }
        let mut refs = self.global_refs.lock().unwrap();
        for block in llmod.diagnostic_globals() {
            let mut current = None;
            for line in block.lines() {
                let line = line.trim();
                let rhs = if line.starts_with('@') {
                    if let Some((lhs, rhs)) = line.split_once(" = ") {
                        current = tokens(lhs, b'@').into_iter().next();
                        rhs
                    } else {
                        line
                    }
                } else {
                    line
                };
                if let Some(name) = &current {
                    refs.entry(name.clone())
                        .or_default()
                        .extend(tokens(rhs, b'@'));
                }
            }
        }
        for targets in refs.values_mut() {
            targets.sort();
            targets.dedup();
        }
        drop(refs);
        self.machines.lock().unwrap().extend(found);
        Ok(())
    }

    pub(crate) fn bind_namespace_getters(
        &self,
        module: &str,
        prefix: &str,
        entries: &[super::NamespaceEntry],
    ) {
        let mut map = self.namespace_getters.lock().unwrap();
        for (i, e) in entries.iter().enumerate() {
            if let Some(&n) = self.export_nodes.get(&(module.to_owned(), e.name.clone())) {
                map.insert(
                    super::helpers::namespace_live_getter_wrapper_symbol(prefix, i),
                    n,
                );
            }
        }
    }

    pub fn machines(&self) -> Vec<MachineFunction> {
        let mut rows = self.machines.lock().unwrap().clone();
        rows.sort_by(|a, b| a.symbol.cmp(&b.symbol).then(a.module.cmp(&b.module)));
        rows
    }

    fn expanded_references(&self, refs: &[String]) -> BTreeSet<String> {
        let globals = self.global_refs.lock().unwrap();
        let mut found = BTreeSet::new();
        let mut work = refs.to_vec();
        while let Some(s) = work.pop() {
            if !found.insert(s.clone()) {
                continue;
            }
            if let Some(targets) = globals.get(&s) {
                work.extend(targets.iter().cloned());
            }
        }
        found
    }

    /// Precision sensitivity includes independently checked generated direct
    /// references. It is not a second dead-code certificate: only the full
    /// conservative HIR solve can declare a body unreachable.
    pub fn precise_nodes(&self) -> Vec<bool> {
        let machines = self.machines();
        let mut by_node = vec![vec![]; self.nodes.len()];
        let mut by_symbol: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, f) in machines.iter().enumerate() {
            if let Some(n) = f.node {
                by_node[n].push(i);
            }
            by_symbol.entry(&f.symbol).or_default().push(i);
        }
        let mut live = self.precise.clone();
        let mut nodes: VecDeque<_> = live
            .iter()
            .enumerate()
            .filter_map(|(n, &v)| v.then_some(n))
            .collect();
        let mut funcs = VecDeque::new();
        let mut seen = vec![false; machines.len()];
        while !nodes.is_empty() || !funcs.is_empty() {
            while let Some(n) = nodes.pop_front() {
                for e in &self.nodes[n].edges {
                    if !e.coarse && !live[e.to] {
                        live[e.to] = true;
                        nodes.push_back(e.to);
                    }
                }
                funcs.extend(by_node[n].iter().copied());
            }
            while let Some(i) = funcs.pop_front() {
                if seen[i] {
                    continue;
                }
                seen[i] = true;
                for s in self.expanded_references(&machines[i].precise_references) {
                    if let Some(n) = self.node_for_symbol(&s) {
                        if self.verdicts[n].reachable && !live[n] {
                            live[n] = true;
                            nodes.push_back(n);
                        }
                    }
                    if let Some(targets) = by_symbol.get(s.as_str()) {
                        funcs.extend(targets.iter().copied());
                    }
                }
            }
        }
        live
    }

    pub fn check(&self) -> Result<()> {
        let machines = self.machines.lock().unwrap();
        let mut errors = BTreeSet::new();
        for f in machines.iter() {
            if f.node.is_some_and(|n| !self.verdicts[n].reachable) {
                continue;
            }
            for target in self.expanded_references(&f.references) {
                if let Some(n) = self.node_for_symbol(&target) {
                    if !self.verdicts[n].reachable {
                        errors.insert(format!(
                            "{} -> {} ({})",
                            f.symbol, target, self.nodes[n].module
                        ));
                    }
                }
            }
        }
        if !errors.is_empty() {
            bail!(
                "PERRY_REACH_REPORT root contract violated ({} generated semantic references):\n{}",
                errors.len(),
                errors.into_iter().take(40).collect::<Vec<_>>().join("\n")
            );
        }
        Ok(())
    }
}

fn solve(
    nodes: &[Node],
    roots: &[(usize, &'static str, bool)],
    coarse: bool,
) -> Vec<Option<String>> {
    let mut result = vec![None; nodes.len()];
    let mut work = VecDeque::new();
    for &(n, reason, c) in roots {
        if (!c || coarse) && result[n].is_none() {
            result[n] = Some(reason.into());
            work.push_back(n);
        }
    }
    while let Some(n) = work.pop_front() {
        for edge in &nodes[n].edges {
            if (!edge.coarse || coarse) && result[edge.to].is_none() {
                result[edge.to] = Some(format!("{} <- {}", edge.reason, nodes[n].label));
                work.push_back(edge.to);
            }
        }
    }
    result
}

fn tokens(line: &str, marker: u8) -> Vec<String> {
    let bytes = line.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            i += usize::from(i < bytes.len());
            continue;
        }
        if bytes[i] != marker {
            i += 1;
            continue;
        }
        let start = i + 1;
        i = start;
        if i < bytes.len() && bytes[i] == b'"' {
            i += 1;
            let s = i;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            out.push(line[s..i].to_string());
            i += usize::from(i < bytes.len());
        } else {
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || b"_$.-".contains(&bytes[i]))
            {
                i += 1;
            }
            if i > start {
                out.push(line[start..i].to_string());
            }
        }
    }
    out
}

fn registration(line: &str) -> bool {
    // Only generated inventories: runtime-observable source operations such as
    // computed-member evaluation and property writes remain ordinary sinks.
    [
        "js_register_function_metadata",
        "js_register_class_declaration",
        "js_register_class_constructor",
        "js_register_class_constructor_with_arity",
        "js_register_constructor",
        "js_register_static_method",
        "js_register_class_static_method",
        "js_gc_register_global_root",
        "js_register_path_init",
        "js_worker_threads_register_entry",
        "js_register_class_static_method_entry",
        "js_register_class_static_getter",
        "js_register_class_static_setter",
        "js_register_class_constructor_flags",
        "js_register_class_method_bind_length",
        "js_register_class_static_method_bind_length",
    ]
    .iter()
    .any(|name| line.contains(&format!("@{name}(")))
}

fn executable_references(lines: &[String]) -> Vec<String> {
    let mut definitions: HashMap<String, (Vec<String>, Vec<String>)> = HashMap::new();
    let mut work = vec![];
    let mut loads = vec![];
    let mut globals = BTreeSet::new();
    for line in lines {
        let line = line.trim();
        if line.starts_with(';') || registration(line) {
            continue;
        }
        let (def, rhs) = line
            .split_once(" = ")
            .filter(|(lhs, _)| lhs.starts_with('%'))
            .map_or((None, line), |(lhs, rhs)| (Some(lhs[1..].to_owned()), rhs));
        let regs = tokens(rhs, b'%');
        let refs = tokens(rhs, b'@');
        if rhs.starts_with("load ") {
            loads.push((regs.clone(), refs.clone()));
        }
        // Calls can have effects even when their returned value is unused.
        let sink = def.is_none()
            || rhs.starts_with("call ")
            || rhs.starts_with("invoke ")
            || rhs.contains(" call ")
            || rhs.contains(" invoke ")
            || rhs.starts_with("atomicrmw ")
            || rhs.starts_with("cmpxchg ");
        if sink {
            work.extend(regs.clone());
            globals.extend(refs.clone());
        }
        if let Some(def) = def {
            definitions.insert(def, (regs, refs));
        }
    }
    for (mut regs, refs) in loads {
        let mut read: BTreeSet<_> = refs.into_iter().collect();
        let mut seen = HashSet::new();
        while let Some(r) = regs.pop() {
            if !seen.insert(r.clone()) {
                continue;
            }
            if let Some((rs, gs)) = definitions.get(&r) {
                regs.extend(rs.iter().cloned());
                read.extend(gs.iter().cloned());
            }
        }
        for g in read {
            if let Some(prefix) = g.strip_prefix("__perry_ns_") {
                globals.insert(format!("__perry_namespace_observed_{prefix}"));
            }
        }
    }
    let mut seen = HashSet::new();
    while let Some(reg) = work.pop() {
        if !seen.insert(reg.clone()) {
            continue;
        }
        if let Some((regs, refs)) = definitions.get(&reg) {
            work.extend(regs.iter().cloned());
            globals.extend(refs.iter().cloned());
        }
    }
    // Function info names have their body's name as prefix. Actual closure
    // allocation references therefore check the body, without rooting unused
    // emitted info/name/source globals.
    globals.into_iter().collect()
}

#[cfg(test)]
mod tests;

#[derive(Clone)]
struct NamespaceSpan {
    function: String,
    module: String,
    block: String,
    start: usize,
    end: usize,
}
thread_local! { static NS_SPANS: std::cell::RefCell<Option<Vec<NamespaceSpan>>> = const {std::cell::RefCell::new(None)}; }
pub(crate) struct ModuleScope(Option<Vec<NamespaceSpan>>);
impl Drop for ModuleScope {
    fn drop(&mut self) {
        NS_SPANS.with(|s| *s.borrow_mut() = self.0.take());
    }
}
pub(crate) fn enter_module() -> ModuleScope {
    ModuleScope(NS_SPANS.with(|s| s.replace(Some(vec![]))))
}
pub(crate) fn namespace_begin(f: &crate::function::LlFunction) -> Option<Vec<(String, usize)>> {
    NS_SPANS.with(|s| s.borrow().is_some()).then(|| {
        f.blocks()
            .iter()
            .map(|b| (b.label.clone(), b.insts().len()))
            .collect()
    })
}
pub(crate) fn namespace_end(
    f: &crate::function::LlFunction,
    module: &str,
    before: Vec<(String, usize)>,
) {
    let before: HashMap<_, _> = before.into_iter().collect();
    NS_SPANS.with(|s| {
        if let Some(spans) = s.borrow_mut().as_mut() {
            for b in f.blocks() {
                let start = before.get(&b.label).copied().unwrap_or(0);
                let end = b.insts().len();
                if start < end {
                    spans.push(NamespaceSpan {
                        function: f.name.clone(),
                        module: module.into(),
                        block: b.label.clone(),
                        start,
                        end,
                    });
                }
            }
        }
    });
}

// One ephemeral whole-program diagnostic per compiler process. Module workers
// take an Arc snapshot; no public CompileOptions/cache input changes are needed.
static ACTIVE: Mutex<Option<std::sync::Arc<Census>>> = Mutex::new(None);
pub struct ProgramScope;
impl Drop for ProgramScope {
    fn drop(&mut self) {
        *ACTIVE.lock().unwrap() = None;
    }
}
pub fn install(census: std::sync::Arc<Census>) -> Result<ProgramScope> {
    let mut active = ACTIVE.lock().unwrap();
    if active.is_some() {
        bail!("reach census already active in this compiler process");
    }
    *active = Some(census);
    Ok(ProgramScope)
}
pub(crate) fn active() -> Option<std::sync::Arc<Census>> {
    ACTIVE.lock().unwrap().clone()
}
