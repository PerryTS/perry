//! NXCALL3: node-core module namespaces under the #10848 no-store fact.
//!
//! A direct call `path.join(…)`, `(0, _path.join)(…)`, `path.posix.join(…)`
//! or `require("fs").readFileSync(…)` binds straight to the native
//! implementation and never reads the namespace member. Like the global
//! namespaces of the main scan, that is only sound while the member still
//! holds the built-in, so the same whole-program pre-scan records every
//! PROGRAM store into a node-core namespace, keyed
//! `("node:<module>", "<member path>")`:
//!
//! - a named or computed member store, `delete`, destructuring assignment
//!   into a member, `Object.assign` / `Object.defineProperty(ies)` /
//!   `Reflect.set` with the namespace as target (the main scan's call arms);
//! - `Object.setPrototypeOf` / `Reflect.setPrototypeOf` on the namespace;
//! - all of these through a program alias the scan follows by name
//!   (`const p = path; p.join = f`, `const { posix } = path`).
//!
//! The member path is dotted (`"join"`, `"promises.readFile"`, `"*"`,
//! `"promises.*"`). `path.posix` is `path` on POSIX hosts (and `path.win32`
//! on Windows), so both prefixes are folded into the root.
//!
//! A namespace handed on as a value (an argument, a stored or returned value,
//! an export or re-export, a dynamic `import()`, a direct `eval`, the
//! CommonJS loader's computed `require`) is NOT a write by itself. Two owner
//! premises say so: "unknown code handed a value never writes members of
//! intrinsics it reaches along that value's chain" (2026-11-19) and P-EVAL,
//! "code built at run time touches program values only through the global
//! object's program keys and the values handed to it" (2026-11-23); the
//! built-in calls rest on the one program-wide fact "no PROGRAM store reaches
//! a built-in prototype, namespace or global binding" (rf-6, 2026-11-19).
//! What program code does with a handed namespace is still a program store
//! the scan must see, but it no longer names the namespace: `function
//! patch(m) { m.basename = f } patch(path)`, `holder.fs.existsSync = f`,
//! `require(name).join = f`, a module that re-exports `path` and a second
//! module that stores into the re-export. So an escape is recorded as
//! `"<prefix>$esc"`, every static-key store onto a target the scan does not
//! resolve is recorded program-wide by key (`PatchedBuiltins::node_stored`), and a call
//! below an escaped prefix is patched when a member name on its path was
//! stored that way somewhere in the program.
//!
//! The bindings are found by NAME, as the main scan's aliases are: an
//! identifier bound anywhere in the module to `require("<node-core>")`, an
//! `import` of one, a member chain of one (`const w = path.win32`), or a
//! destructured member of one (`const { promises: fsp } = require("fs")`),
//! plus the global `process`. A same-named unrelated binding can only add a
//! write that never happens: a generic call, never a wrong one.
//!
//! Not seen (no solver verdict covers a native-module call, and there is no
//! runtime `Pristine` guard): a store with a computed key, or an
//! `Object.assign` with a non-literal source, onto a target the scan does not
//! resolve, after the namespace was handed on (`function f(o, k, v) { o[k] =
//! v } f(path, "join", g)`); a dynamic-key store into the global object or
//! non-constant dynamic code (P-EVAL) does not reach these namespaces.

use std::collections::{BTreeMap, BTreeSet};

use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

use super::{is_global_object, peel, static_member_name, str_arg, ANY_MEMBER};

/// A node-core namespace value: the module key and the member path below it.
pub(crate) type NsPath = (String, String);

/// Identifiers bound to a node-core namespace (or a member of one) by name.
pub(crate) type NodeBindings = BTreeMap<String, NsPath>;

/// `("node:<top>", <dotted rest>)` for a node-core module specifier
/// (`"path"`, `"node:fs/promises"`, the lowering's `"process.namespace"`).
pub(crate) fn module_key(spec: &str) -> Option<NsPath> {
    let m = spec.strip_prefix("node:").unwrap_or(spec);
    let m = m.strip_suffix(".namespace").unwrap_or(m);
    if !crate::ir::is_node_builtin_module(m) {
        return None;
    }
    let (top, rest) = m.split_once('/').unwrap_or((m, ""));
    let key = format!("node:{top}");
    let rest = canonical(&key, &rest.replace('/', "."));
    Some((key, rest))
}

/// Fold the `path.posix` / `path.win32` prefixes into the `path` root.
fn canonical(key: &str, dotted: &str) -> String {
    let mut d = dotted;
    if key == "node:path" {
        loop {
            d = match d {
                "posix" | "win32" => "",
                _ => match d
                    .strip_prefix("posix.")
                    .or_else(|| d.strip_prefix("win32."))
                {
                    Some(rest) => rest,
                    None => break,
                },
            };
        }
    }
    d.to_string()
}

fn join(prefix: &str, member: &str) -> String {
    if prefix.is_empty() {
        member.to_string()
    } else {
        format!("{prefix}.{member}")
    }
}

/// The member path `prefix.member` (`prefix.*` for an unknown key), folded.
pub(crate) fn entry(ns: &NsPath, member: Option<&str>) -> (String, String) {
    let m = join(&ns.1, member.unwrap_or(ANY_MEMBER));
    (ns.0.clone(), canonical(&ns.0, &m))
}

/// Does a recorded member path cover a call of member path `full`?
fn covers(recorded: &str, full: &str) -> bool {
    match recorded.strip_suffix(ANY_MEMBER) {
        Some(prefix) => full.starts_with(prefix),
        None => {
            full == recorded
                || full
                    .strip_prefix(recorded)
                    .is_some_and(|r| r.starts_with('.'))
        }
    }
}

/// Member suffix of a namespace handed on as a value (see the module docs).
pub(crate) const ESCAPE: &str = "$esc";

/// Does an escape of `prefix` reach call path `full` through a member name
/// some unresolved program store wrote (`PatchedBuiltins::node_stored`)?
fn escape_reaches(stored: &BTreeSet<String>, prefix: &str, full: &str) -> bool {
    let Some(rest) = full.strip_prefix(prefix) else {
        return false;
    };
    rest.split('.')
        .filter(|seg| !seg.is_empty())
        .any(|seg| stored.contains(seg))
}

/// An expression that creates its value (`{}`, `[]`, a function or class,
/// `new C()`, a literal): a store onto it never reaches a namespace.
pub(crate) fn fresh_value(e: &ast::Expr) -> bool {
    matches!(
        peel(e),
        ast::Expr::Object(_)
            | ast::Expr::Array(_)
            | ast::Expr::Fn(_)
            | ast::Expr::Arrow(_)
            | ast::Expr::Class(_)
            | ast::Expr::New(_)
            | ast::Expr::Lit(_)
            | ast::Expr::Tpl(_)
    )
}

/// May the program replace member path `path` of node-core module `module`
/// (or a namespace on the way to it)? `module` is the lowering's module name.
pub(crate) fn member_patched(
    set: &super::PatchedBuiltins,
    module: &str,
    path: &[&str],
) -> bool {
    let Some(ns) = module_key(module) else {
        return false;
    };
    let mut full = ns.1.clone();
    for p in path {
        full = join(&full, p);
    }
    let full = canonical(&ns.0, &full);
    set.range((ns.0.clone(), String::new())..)
        .take_while(|(k, _)| *k == ns.0)
        .any(|(_, recorded)| match recorded.strip_suffix(ESCAPE) {
            Some(prefix) => escape_reaches(&set.node_stored, prefix, &full),
            None => covers(recorded, &full),
        })
}

/// `require("<lit>")` with exactly one literal argument.
fn require_literal(e: &ast::Expr) -> Option<String> {
    let ast::Expr::Call(call) = peel(e) else {
        return None;
    };
    let ast::Callee::Expr(callee) = &call.callee else {
        return None;
    };
    match peel(callee) {
        ast::Expr::Ident(id) if id.sym == "require" && call.args.len() == 1 => {
            str_arg(&call.args[0].expr)
        }
        _ => None,
    }
}

/// The node-core namespace value `e` denotes, if any: a bound identifier, the
/// global `process`, `require("<node-core>")`, or a static member chain of one.
pub(crate) fn resolve(e: &ast::Expr, bindings: &NodeBindings) -> Option<NsPath> {
    match peel(e) {
        ast::Expr::Ident(id) => bindings
            .get(id.sym.as_ref())
            .cloned()
            .or_else(|| (id.sym == "process").then(|| ("node:process".to_string(), String::new()))),
        ast::Expr::Member(m) => {
            let name = static_member_name(&m.prop)?;
            if is_global_object(&m.obj) {
                return (name == "process").then(|| ("node:process".to_string(), String::new()));
            }
            let ns = resolve(&m.obj, bindings)?;
            Some(entry(&ns, Some(&name)))
        }
        other => module_key(&require_literal(other)?),
    }
}

fn import_name(n: &ast::ModuleExportName) -> Option<String> {
    match n {
        ast::ModuleExportName::Ident(i) => Some(i.sym.to_string()),
        ast::ModuleExportName::Str(s) => s.value.as_str().map(str::to_string),
    }
}

/// Collects the module's node-core namespace bindings, in source order.
pub(crate) struct BindingCollector<'a> {
    pub(crate) bindings: &'a mut NodeBindings,
}

impl BindingCollector<'_> {
    fn bind_pat(&mut self, pat: &ast::Pat, ns: &NsPath) {
        match pat {
            ast::Pat::Ident(b) => {
                self.bindings.insert(b.id.sym.to_string(), ns.clone());
            }
            ast::Pat::Object(o) => {
                for p in &o.props {
                    match p {
                        ast::ObjectPatProp::Assign(a) => {
                            let k = a.key.sym.to_string();
                            self.bindings.insert(k.clone(), entry(ns, Some(&k)));
                        }
                        ast::ObjectPatProp::KeyValue(kv) => {
                            if let Some(k) = super::prop_name_key(&kv.key) {
                                self.bind_pat(&kv.value, &entry(ns, Some(&k)));
                            }
                        }
                        ast::ObjectPatProp::Rest(_) => {}
                    }
                }
            }
            _ => {}
        }
    }
}

impl Visit for BindingCollector<'_> {
    fn visit_import_decl(&mut self, import: &ast::ImportDecl) {
        let Some(src) = import.src.value.as_str() else {
            return;
        };
        if import.type_only {
            return;
        }
        let Some(ns) = module_key(src) else {
            return;
        };
        for s in &import.specifiers {
            match s {
                ast::ImportSpecifier::Default(d) => {
                    self.bindings.insert(d.local.sym.to_string(), ns.clone());
                }
                ast::ImportSpecifier::Namespace(n) => {
                    self.bindings.insert(n.local.sym.to_string(), ns.clone());
                }
                ast::ImportSpecifier::Named(n) if !n.is_type_only => {
                    let imported = n
                        .imported
                        .as_ref()
                        .and_then(import_name)
                        .unwrap_or_else(|| n.local.sym.to_string());
                    let path = if imported == "default" {
                        ns.clone()
                    } else {
                        entry(&ns, Some(&imported))
                    };
                    self.bindings.insert(n.local.sym.to_string(), path);
                }
                ast::ImportSpecifier::Named(_) => {}
            }
        }
    }

    fn visit_ts_import_equals_decl(&mut self, d: &ast::TsImportEqualsDecl) {
        if let ast::TsModuleRef::TsExternalModuleRef(r) = &d.module_ref {
            if let Some(ns) = r.expr.value.as_str().and_then(module_key) {
                self.bindings.insert(d.id.sym.to_string(), ns);
            }
        }
    }

    fn visit_var_declarator(&mut self, d: &ast::VarDeclarator) {
        if let Some(init) = &d.init {
            if let Some(ns) = resolve(init, self.bindings) {
                self.bind_pat(&d.name, &ns);
            }
        }
        d.visit_children_with(self);
    }
}

/// Member suffix of an escape inside the CommonJS wrapper's open-program arm
/// (`if (__perry_cjs_open()) { if (specifier === 'path') return _path; }`:
/// the loader hands the namespace to a computed `require`). It counts only
/// when the program is open ([`promote_open_escapes`]); a closed program
/// drops those arms (`cjs_link::resolve_open_sites`).
pub(crate) const OPEN_ESCAPE: &str = "$open";

/// Turn every open-arm escape into an escape (`"<prefix>$esc"`). True when
/// that added an entry.
pub(crate) fn promote_open_escapes(set: &mut BTreeSet<(String, String)>) -> bool {
    let open: Vec<(String, String)> = set
        .iter()
        .filter(|(k, m)| k.starts_with("node:") && m.ends_with(OPEN_ESCAPE))
        .map(|(k, m)| {
            (
                k.clone(),
                format!("{}{ESCAPE}", &m[..m.len() - OPEN_ESCAPE.len()]),
            )
        })
        .collect();
    let mut grew = false;
    for e in open {
        grew |= set.insert(e);
    }
    grew
}

fn is_open_test(e: &ast::Expr) -> bool {
    let ast::Expr::Call(call) = peel(e) else {
        return false;
    };
    matches!(&call.callee, ast::Callee::Expr(c)
        if matches!(peel(c), ast::Expr::Ident(id) if id.sym == "__perry_cjs_open"))
        && call.args.is_empty()
}

/// `Object.assign` and the other writers whose stores the main scan records
/// per key, so that their target is not also an escape.
fn modeled_writer(callee: &ast::Expr) -> bool {
    let ast::Expr::Member(m) = peel(callee) else {
        return false;
    };
    let ast::Expr::Ident(owner) = peel(&m.obj) else {
        return false;
    };
    matches!(
        (owner.sym.as_ref(), static_member_name(&m.prop).as_deref()),
        (
            "Object",
            Some("assign" | "defineProperty" | "defineProperties" | "setPrototypeOf")
        ) | ("Reflect", Some("defineProperty" | "set" | "setPrototypeOf"))
    )
}

/// Records node-core namespaces handed on as values (see the module docs):
/// an escape is not a write; it only lets an unresolved program store reach
/// the namespace.
pub(crate) struct EscapeScan<'a> {
    pub(crate) bindings: &'a NodeBindings,
    pub(crate) out: &'a mut BTreeSet<(String, String)>,
    /// Inside the CommonJS wrapper's `if (__perry_cjs_open()) { … }` arm.
    pub(crate) open_arm: bool,
}

impl EscapeScan<'_> {
    fn escape(&mut self, ns: &NsPath) {
        let member = if self.open_arm { OPEN_ESCAPE } else { ESCAPE };
        self.out.insert(entry(ns, Some(member)));
    }
    /// Visit `e` unless it is itself a namespace value (only read here).
    fn read_only(&mut self, e: &ast::Expr) {
        // A resolved chain has only static keys: nothing below it runs.
        if resolve(e, self.bindings).is_none() {
            e.visit_with(self);
        }
    }
}

impl Visit for EscapeScan<'_> {
    fn visit_expr(&mut self, e: &ast::Expr) {
        if let Some(ns) = resolve(e, self.bindings) {
            self.escape(&ns);
        }
        e.visit_children_with(self);
    }
    fn visit_member_expr(&mut self, m: &ast::MemberExpr) {
        self.read_only(&m.obj);
        m.prop.visit_with(self);
    }
    fn visit_callee(&mut self, c: &ast::Callee) {
        match c {
            ast::Callee::Expr(e) => self.read_only(e),
            _ => c.visit_children_with(self),
        }
    }
    fn visit_call_expr(&mut self, call: &ast::CallExpr) {
        match &call.callee {
            // `import("fs")` hands the namespace to the awaiting code.
            ast::Callee::Import(_) => {
                if let Some(ns) = call
                    .args
                    .first()
                    .and_then(|a| str_arg(&a.expr))
                    .and_then(|s| module_key(&s))
                {
                    self.escape(&ns);
                }
            }
            // A direct `eval` sees every binding of its module.
            ast::Callee::Expr(e) if matches!(peel(e), ast::Expr::Ident(id) if id.sym == "eval") => {
                let all: Vec<NsPath> = self.bindings.values().cloned().collect();
                for ns in &all {
                    self.escape(ns);
                }
                self.escape(&("node:process".to_string(), String::new()));
            }
            // The writers the main scan models key by key (`Object.assign`,
            // `defineProperty`, …) only read their target here.
            ast::Callee::Expr(e) if modeled_writer(e) => {
                if let Some((first, rest)) = call.args.split_first() {
                    if first.spread.is_none() {
                        self.read_only(&first.expr);
                        rest.visit_with(self);
                        return;
                    }
                }
            }
            _ => {}
        }
        call.visit_children_with(self);
    }
    fn visit_new_expr(&mut self, n: &ast::NewExpr) {
        self.read_only(&n.callee);
        n.args.visit_with(self);
    }
    fn visit_if_stmt(&mut self, s: &ast::IfStmt) {
        if !is_open_test(&s.test) {
            return s.visit_children_with(self);
        }
        let outer = std::mem::replace(&mut self.open_arm, true);
        s.cons.visit_with(self);
        self.open_arm = outer;
        s.alt.visit_with(self);
    }
    fn visit_unary_expr(&mut self, u: &ast::UnaryExpr) {
        if u.op == ast::UnaryOp::TypeOf {
            self.read_only(&u.arg);
        } else {
            u.visit_children_with(self);
        }
    }
    fn visit_bin_expr(&mut self, b: &ast::BinExpr) {
        if b.op == ast::BinaryOp::InstanceOf {
            b.left.visit_with(self);
            self.read_only(&b.right);
        } else {
            b.visit_children_with(self);
        }
    }
    fn visit_expr_stmt(&mut self, s: &ast::ExprStmt) {
        // `require("path");` evaluates and drops the namespace.
        self.read_only(&s.expr);
    }
    fn visit_var_declarator(&mut self, d: &ast::VarDeclarator) {
        // `const p = require("path")` / `const w = path.win32` bind a name the
        // scan follows; `const { join } = path` reads members.
        d.name.visit_with(self);
        if let Some(init) = &d.init {
            match &d.name {
                ast::Pat::Ident(_) | ast::Pat::Object(_) => self.read_only(init),
                _ => init.visit_with(self),
            }
        }
    }
    fn visit_assign_expr(&mut self, a: &ast::AssignExpr) {
        // `({ join } = path)` reads members.
        a.left.visit_with(self);
        match &a.left {
            ast::AssignTarget::Pat(ast::AssignTargetPat::Object(_)) => self.read_only(&a.right),
            _ => a.right.visit_with(self),
        }
    }
    fn visit_prop(&mut self, p: &ast::Prop) {
        if let ast::Prop::Shorthand(id) = p {
            if let Some(ns) = self.bindings.get(id.sym.as_ref()).cloned() {
                self.escape(&ns);
            } else if id.sym == "process" {
                self.escape(&("node:process".to_string(), String::new()));
            }
        }
        p.visit_children_with(self);
    }
    fn visit_export_named_specifier(&mut self, s: &ast::ExportNamedSpecifier) {
        if let Some(name) = import_name(&s.orig) {
            if let Some(ns) = self.bindings.get(&name).cloned() {
                self.escape(&ns);
            }
        }
    }
    fn visit_named_export(&mut self, e: &ast::NamedExport) {
        // `export { posix } from "path"` re-exports the namespace's members.
        if let Some(ns) = e
            .src
            .as_ref()
            .and_then(|s| s.value.as_str())
            .and_then(module_key)
        {
            for s in &e.specifiers {
                let member = match s {
                    ast::ExportSpecifier::Named(n) => import_name(&n.orig),
                    _ => None,
                };
                match member.as_deref() {
                    Some("default") | None => self.escape(&ns),
                    Some(m) => self.escape(&entry(&ns, Some(m))),
                }
            }
            return;
        }
        e.visit_children_with(self);
    }
    fn visit_export_all(&mut self, e: &ast::ExportAll) {
        if let Some(ns) = e.src.value.as_str().and_then(module_key) {
            self.escape(&ns);
        }
    }
}
