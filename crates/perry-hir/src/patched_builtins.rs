//! #10848: program-wide record of built-in members the program REPLACES.
//!
//! Direct member calls on the built-in namespaces (`console.log(…)`,
//! `Math.max(…)`, `JSON.stringify(…)`) are devirtualized at lowering: the call
//! binds straight to the intrinsic implementation and never re-reads the
//! property. That is only sound while the property still holds the built-in.
//! Replacing it is ordinary JS — spies, polyfills, loggers, OpenTUI's console
//! capture — and the write itself lands (every *read* path already sees the
//! replacement), so the devirtualized call site was the one place that
//! silently kept running the original.
//!
//! The fix is a whole-program pre-scan: every module's AST is searched for
//! writes to a built-in member (`console.log = f`, `Math["max"] = f`,
//! `console[m] = f`, `Object.defineProperty(Math, "max", …)`,
//! `globalThis.console = {…}`, …). The union over the program is installed per
//! lowering thread (the driver re-lowers when a later module adds a patch that
//! an earlier-lowered module missed), and a direct call whose member is in the
//! set lowers as an ordinary dynamic property-get-then-call instead of the
//! intrinsic. Programs that never patch a built-in are unaffected: the set is
//! empty and every fast path stays.
//!
//! Keys are `(namespace, member)`: `member` is the property name, or
//! [`ANY_MEMBER`] when the write's key is not static (`console[m] = f`) or the
//! whole namespace object was replaced (`globalThis.console = {…}`).
//!
//! #11394: the same pre-scan records writes onto the PROTOTYPE of the builtins
//! whose instance methods are dispatched by name (`Array.prototype.push = f`,
//! `Map.prototype.get = f`, `Function.prototype.bind = f`), keyed
//! `("Array.prototype", "push")`. A proven receiver folds such a call to an
//! intrinsic (`ArrayPush`, `MapGet`) and a dynamic one matches the name in the
//! runtime's native dispatcher; neither reads the prototype slot. A call whose
//! method name is patched on any of these prototypes lowers instead to the
//! runtime entry that performs the property lookup first
//! (`js_native_call_method_patched_proto`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

/// Member wildcard: a dynamic-key write, or the whole namespace replaced.
pub const ANY_MEMBER: &str = "*";

/// The set of patched `(namespace, member)` pairs.
pub type PatchedBuiltins = BTreeSet<(String, String)>;

/// Global namespace objects whose direct member calls lowering devirtualizes.
/// A patch to any other global object's member is already honored, because
/// its calls go through ordinary property dispatch.
const BUILTIN_NAMESPACES: &[&str] = &[
    "console", "Math", "JSON", "Reflect", "Object", "Number", "String", "Array", "Promise", "Date",
    "Symbol", "BigInt", "Atomics", "Intl", "Boolean",
];

/// Identifiers that name the global object.
const GLOBAL_OBJECT_NAMES: &[&str] = &["globalThis", "global", "window", "self"];

/// Builtins whose `prototype` methods the runtime answers by NAME on an
/// instance (arrays, Map/Set, functions), so a replacement on the prototype is
/// only honored if the call site is routed through a lookup (#11394).
const PROTOTYPE_OWNERS: &[&str] = &["Array", "Map", "Set", "Function"];

/// Suffix of a [`PatchedBuiltins`] key naming a builtin's prototype object.
const PROTOTYPE_SUFFIX: &str = ".prototype";

pub fn is_builtin_namespace(name: &str) -> bool {
    BUILTIN_NAMESPACES.contains(&name)
}

fn peel(mut e: &ast::Expr) -> &ast::Expr {
    loop {
        e = match e {
            ast::Expr::Paren(x) => &x.expr,
            ast::Expr::TsAs(x) => &x.expr,
            ast::Expr::TsNonNull(x) => &x.expr,
            ast::Expr::TsSatisfies(x) => &x.expr,
            ast::Expr::TsTypeAssertion(x) => &x.expr,
            ast::Expr::TsConstAssertion(x) => &x.expr,
            _ => return e,
        };
    }
}

/// Static property name of a member access (`o.p` / `o["p"]` / `o[7]`).
pub fn static_member_name(prop: &ast::MemberProp) -> Option<String> {
    match prop {
        ast::MemberProp::Ident(p) => Some(p.sym.to_string()),
        ast::MemberProp::Computed(c) => str_arg(&c.expr),
        ast::MemberProp::PrivateName(_) => None,
    }
}

fn is_global_object(e: &ast::Expr) -> bool {
    matches!(peel(e), ast::Expr::Ident(id) if GLOBAL_OBJECT_NAMES.contains(&id.sym.as_ref()))
}

/// `N` or `globalThis.N` (and the `global`/`window`/`self` spellings) for a
/// builtin namespace `N`.
fn namespace_receiver(e: &ast::Expr) -> Option<String> {
    match peel(e) {
        ast::Expr::Ident(id) if is_builtin_namespace(id.sym.as_ref()) => Some(id.sym.to_string()),
        ast::Expr::Member(m) if is_global_object(&m.obj) => {
            static_member_name(&m.prop).filter(|n| is_builtin_namespace(n))
        }
        _ => None,
    }
}

/// `C.prototype` or `globalThis.C.prototype` for a builtin `C` in
/// [`PROTOTYPE_OWNERS`], or an identifier in `aliases` bound to one
/// (`const AP = Array.prototype; AP.push = f`); answers the owner name.
fn prototype_receiver(e: &ast::Expr, aliases: &BTreeMap<String, String>) -> Option<String> {
    let m = match peel(e) {
        ast::Expr::Ident(id) => return aliases.get(id.sym.as_ref()).cloned(),
        ast::Expr::Member(m) => m,
        _ => return None,
    };
    if static_member_name(&m.prop).as_deref() != Some("prototype") {
        return None;
    }
    let owner = match peel(&m.obj) {
        ast::Expr::Ident(id) => id.sym.to_string(),
        ast::Expr::Member(g) if is_global_object(&g.obj) => static_member_name(&g.prop)?,
        _ => return None,
    };
    PROTOTYPE_OWNERS.contains(&owner.as_str()).then_some(owner)
}

fn prop_name_key(p: &ast::PropName) -> Option<String> {
    match p {
        ast::PropName::Ident(i) => Some(i.sym.to_string()),
        ast::PropName::Str(s) => s.value.as_str().map(str::to_string),
        _ => None,
    }
}

/// Static keys an object literal defines, or `None` if any key is dynamic.
fn object_literal_keys(e: &ast::Expr) -> Option<Vec<String>> {
    let ast::Expr::Object(obj) = peel(e) else {
        return None;
    };
    let mut keys = Vec::new();
    for prop in &obj.props {
        let ast::PropOrSpread::Prop(prop) = prop else {
            return None;
        };
        let key = match prop.as_ref() {
            ast::Prop::Shorthand(i) => Some(i.sym.to_string()),
            ast::Prop::KeyValue(kv) => prop_name_key(&kv.key),
            ast::Prop::Method(m) => prop_name_key(&m.key),
            ast::Prop::Getter(g) => prop_name_key(&g.key),
            ast::Prop::Setter(s) => prop_name_key(&s.key),
            ast::Prop::Assign(_) => None,
        };
        keys.push(key?);
    }
    Some(keys)
}

fn is_index_key(key: &str) -> bool {
    !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit())
}

/// A literal property key: a string, or an integer-valued number (`[7]`,
/// `defineProperty(o, 7, …)` — an index, which no method call can name).
fn str_arg(e: &ast::Expr) -> Option<String> {
    match peel(e) {
        ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str().map(str::to_string),
        ast::Expr::Lit(ast::Lit::Num(n))
            if n.value.fract() == 0.0 && n.value.abs() < 9_007_199_254_740_992.0 =>
        {
            Some(format!("{}", n.value as i64))
        }
        _ => None,
    }
}

/// `[Symbol.iterator]` etc. as a key that can never equal a method name, so a
/// patched well-known symbol is not read as "any member" (which would also
/// mark `push` patched and bypass spread's iterator protocol, #11772).
fn well_known_symbol_key(prop: &ast::MemberProp) -> Option<String> {
    let ast::MemberProp::Computed(c) = prop else {
        return None;
    };
    let ast::Expr::Member(m) = peel(&c.expr) else {
        return None;
    };
    match peel(&m.obj) {
        ast::Expr::Ident(id) if id.sym.as_ref() == "Symbol" => {}
        _ => return None,
    }
    match &m.prop {
        ast::MemberProp::Ident(p) => Some(format!("@@{}", p.sym)),
        _ => None,
    }
}

struct Scanner<'a> {
    out: &'a mut PatchedBuiltins,
    /// Identifiers bound to a builtin prototype anywhere in the module, by
    /// NAME and not by scope: a same-named binding elsewhere can only add a
    /// patch that never happens, which costs a lookup and never a wrong call.
    proto_aliases: BTreeMap<String, String>,
    /// Identifiers bound to a builtin namespace (`const M: any = Math`), by
    /// name for the same reason: a write through one patches the namespace.
    ns_aliases: BTreeMap<String, String>,
}

/// Collects `const AP = Array.prototype`-style aliases ahead of the scan, so a
/// write through one is seen wherever the declaration sits.
struct AliasCollector<'a> {
    aliases: &'a mut BTreeMap<String, String>,
    ns_aliases: &'a mut BTreeMap<String, String>,
}

impl Visit for AliasCollector<'_> {
    fn visit_var_declarator(&mut self, d: &ast::VarDeclarator) {
        if let (ast::Pat::Ident(name), Some(init)) = (&d.name, &d.init) {
            if let Some(owner) = prototype_receiver(init, &BTreeMap::new()) {
                self.aliases.insert(name.id.sym.to_string(), owner);
            } else if let Some(ns) = namespace_receiver(init) {
                self.ns_aliases.insert(name.id.sym.to_string(), ns);
            }
        }
        d.visit_children_with(self);
    }
}

impl Scanner<'_> {
    fn add(&mut self, namespace: String, member: Option<String>) {
        let member = member.unwrap_or_else(|| ANY_MEMBER.to_string());
        self.out.insert((namespace, member));
    }

    /// A write of property `member` (None = dynamic key) onto `target`.
    fn record_write(&mut self, target: &ast::Expr, member: Option<String>) {
        if let Some(ns) = namespace_receiver(target).or_else(|| match peel(target) {
            ast::Expr::Ident(id) => self.ns_aliases.get(id.sym.as_ref()).cloned(),
            _ => None,
        }) {
            self.add(ns, member);
        } else if let Some(owner) = prototype_receiver(target, &self.proto_aliases) {
            // An index (`Array.prototype[7]`) is an element, not a method any
            // call site names; the array runtime already honors it (#6981).
            if member.as_deref().is_some_and(is_index_key) {
                return;
            }
            self.add(format!("{owner}{PROTOTYPE_SUFFIX}"), member);
        } else if is_global_object(target) {
            // `globalThis.console = {…}` replaces the whole namespace.
            // A dynamic key (`globalThis[k] = v`, `Object.assign(global,
            // shims)`) is deliberately NOT read as "every namespace replaced":
            // shim and bundle preludes install globals that way, and it would
            // strip every Math/JSON/console fast path from those programs.
            if let Some(n) = member.filter(|n| is_builtin_namespace(n)) {
                self.add(n, None);
            }
        }
    }

    fn record_member_target(&mut self, m: &ast::MemberExpr) {
        let key = static_member_name(&m.prop).or_else(|| well_known_symbol_key(&m.prop));
        self.record_write(&m.obj, key);
    }

    fn record_pat(&mut self, pat: &ast::Pat) {
        match pat {
            ast::Pat::Expr(e) => {
                if let ast::Expr::Member(m) = peel(e) {
                    self.record_member_target(m);
                }
            }
            ast::Pat::Array(a) => {
                for p in a.elems.iter().flatten() {
                    self.record_pat(p);
                }
            }
            ast::Pat::Object(o) => {
                for p in &o.props {
                    match p {
                        ast::ObjectPatProp::KeyValue(kv) => self.record_pat(&kv.value),
                        ast::ObjectPatProp::Rest(r) => self.record_pat(&r.arg),
                        ast::ObjectPatProp::Assign(_) => {}
                    }
                }
            }
            ast::Pat::Assign(a) => self.record_pat(&a.left),
            ast::Pat::Rest(r) => self.record_pat(&r.arg),
            _ => {}
        }
    }
}

impl Visit for Scanner<'_> {
    fn visit_assign_expr(&mut self, a: &ast::AssignExpr) {
        match &a.left {
            ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(m)) => {
                self.record_member_target(m)
            }
            ast::AssignTarget::Simple(ast::SimpleAssignTarget::Paren(p)) => {
                if let ast::Expr::Member(m) = peel(&p.expr) {
                    self.record_member_target(m);
                }
            }
            ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(id))
                if is_builtin_namespace(id.id.sym.as_ref()) =>
            {
                // Sloppy `console = {…}` rebinding the global.
                self.add(id.id.sym.to_string(), None);
            }
            ast::AssignTarget::Pat(p) => match p {
                ast::AssignTargetPat::Array(a) => {
                    for e in a.elems.iter().flatten() {
                        self.record_pat(e);
                    }
                }
                ast::AssignTargetPat::Object(o) => {
                    for p in &o.props {
                        if let ast::ObjectPatProp::KeyValue(kv) = p {
                            self.record_pat(&kv.value);
                        }
                    }
                }
                ast::AssignTargetPat::Invalid(_) => {}
            },
            _ => {}
        }
        a.visit_children_with(self);
    }

    fn visit_call_expr(&mut self, call: &ast::CallExpr) {
        call.visit_children_with(self);
        let ast::Callee::Expr(callee) = &call.callee else {
            return;
        };
        let ast::Expr::Member(m) = peel(callee) else {
            return;
        };
        let Some(method) = static_member_name(&m.prop) else {
            return;
        };
        let owner = match peel(&m.obj) {
            ast::Expr::Ident(id) => id.sym.to_string(),
            _ => return,
        };
        let arg = |i: usize| call.args.get(i).map(|a| (a.spread.is_some(), &*a.expr));
        let Some((false, target)) = arg(0) else {
            return;
        };
        match (owner.as_str(), method.as_str()) {
            ("Object" | "Reflect", "defineProperty") | ("Reflect", "set") => {
                let key = match arg(1) {
                    Some((false, k)) => str_arg(k),
                    _ => None,
                };
                self.record_write(target, key);
            }
            ("Object", "defineProperties" | "assign") => {
                // `Object.assign(Math, a, b)` — every source's keys land.
                let sources: Vec<_> = call.args.iter().skip(1).collect();
                if sources.is_empty() {
                    return;
                }
                for src in sources {
                    match (src.spread.is_none())
                        .then(|| object_literal_keys(&src.expr))
                        .flatten()
                    {
                        Some(keys) => {
                            for k in keys {
                                self.record_write(target, Some(k));
                            }
                        }
                        None => self.record_write(target, None),
                    }
                }
            }
            _ => {}
        }
    }
}

/// Collect every built-in member write in `module`.
pub fn scan_module(module: &ast::Module, out: &mut PatchedBuiltins) {
    let mut proto_aliases = BTreeMap::new();
    let mut ns_aliases = BTreeMap::new();
    module.visit_with(&mut AliasCollector {
        aliases: &mut proto_aliases,
        ns_aliases: &mut ns_aliases,
    });
    module.visit_with(&mut Scanner {
        out,
        proto_aliases,
        ns_aliases,
    });
}

thread_local! {
    /// The program-wide union, installed by the driver before each module's
    /// lowering and cleared after (rayon-safe: one copy per worker thread).
    static PATCHED: std::cell::RefCell<Option<Arc<PatchedBuiltins>>> =
        const { std::cell::RefCell::new(None) };
}

pub fn set_patched_builtins(set: Arc<PatchedBuiltins>) {
    PATCHED.with(|p| *p.borrow_mut() = if set.is_empty() { None } else { Some(set) });
}

pub fn clear_patched_builtins() {
    PATCHED.with(|p| *p.borrow_mut() = None);
}

fn with_set<R>(f: impl FnOnce(&PatchedBuiltins) -> R) -> Option<R> {
    PATCHED.with(|p| p.borrow().as_deref().map(f))
}

/// Was `ns.member` (for a builtin namespace `ns`) replaced anywhere?
pub fn namespace_member_patched(ns: &str, member: &str) -> bool {
    with_set(|s| {
        s.contains(&(ns.to_string(), member.to_string()))
            || s.contains(&(ns.to_string(), ANY_MEMBER.to_string()))
    })
    .unwrap_or(false)
}

/// Was a method named `member` written onto the prototype of a builtin whose
/// instance methods are dispatched by name (#11394)? Deliberately not keyed by
/// the receiver's kind: a call site rarely proves it, and the runtime entry it
/// selects consults the actual receiver.
pub fn prototype_method_patched(member: &str) -> bool {
    with_set(|s| {
        s.iter()
            .any(|(ns, m)| ns.ends_with(PROTOTYPE_SUFFIX) && (m == member || m == ANY_MEMBER))
    })
    .unwrap_or(false)
}

/// The method names [`prototype_method_patched`] answers for, as the sorted
/// list codegen is handed ([`ANY_MEMBER`] when a write's key was dynamic).
pub fn patched_prototype_methods(set: &PatchedBuiltins) -> Vec<String> {
    let names: BTreeSet<&str> = set
        .iter()
        .filter(|(ns, _)| ns.ends_with(PROTOTYPE_SUFFIX))
        .map(|(_, m)| m.as_str())
        .collect();
    names.into_iter().map(str::to_string).collect()
}

#[cfg(test)]
#[path = "patched_builtins_tests.rs"]
mod tests;
