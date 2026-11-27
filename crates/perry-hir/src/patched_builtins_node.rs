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
//! CommonJS loader's computed `require`) is NOT a write. Two owner premises
//! say so: "unknown code handed a value never writes members of intrinsics it
//! reaches along that value's chain" (2026-11-19) and P-EVAL, "code built at
//! run time touches program values only through the global object's program
//! keys and the values handed to it" (2026-11-23).
//!
//! The bindings are found by NAME, as the main scan's aliases are: an
//! identifier bound anywhere in the module to `require("<node-core>")`, an
//! `import` of one, a member chain of one (`const w = path.win32`), or a
//! destructured member of one (`const { promises: fsp } = require("fs")`),
//! plus the global `process`. A same-named unrelated binding can only add a
//! write that never happens: a generic call, never a wrong one.
//!
//! Not seen, the same gap the #10848 globals scan has for computed keys: a
//! program store onto a target the scan does not resolve to a namespace, an
//! alias, a member chain, a literal `require` or a destructured binding
//! (`function patch(m) { m.join = f } patch(path)`, `holder.fs.existsSync =
//! f`, `require(name).join = f`, a store into a re-export another module
//! imports), and a computed-key store after the namespace was handed on. The
//! precise answer is the solver's program-wide store fact (rf-6, 2026-11-19:
//! "no PROGRAM store reaches a built-in prototype, namespace or global
//! binding"), a follow-up; no runtime `Pristine` guard covers these calls.

use std::collections::BTreeMap;

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

/// May the program replace member path `path` of node-core module `module`
/// (or a namespace on the way to it)? `module` is the lowering's module name.
pub(crate) fn member_patched(set: &super::PatchedBuiltins, module: &str, path: &[&str]) -> bool {
    patch_entry(set, module, path).is_some()
}

/// The recorded write that makes member path `path` of `module` patched.
pub(crate) fn patch_entry(
    set: &super::PatchedBuiltins,
    module: &str,
    path: &[&str],
) -> Option<(String, String)> {
    let ns = module_key(module)?;
    let mut full = ns.1.clone();
    for p in path {
        full = join(&full, p);
    }
    let full = canonical(&ns.0, &full);
    set.range((ns.0.clone(), String::new())..)
        .take_while(|(k, _)| *k == ns.0)
        .find(|(_, recorded)| covers(recorded, &full))
        .cloned()
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
