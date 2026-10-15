//! The whole-program prototype-write census (object model step 3,
//! `OBJECT_MODEL_2026-10-13.md` §3.6).
//!
//! A prototype slot written exactly once in the program is a constant: a
//! method call on a receiver whose family set is proven then binds the method
//! body directly, with no load for dispatch. "Written exactly once" is a
//! compile-time fact of the whole program's source, collected here by the same
//! module pre-scan that records builtin-prototype patches
//! ([`super::scan_module`]), and folded into one program-wide
//! [`ProtoCensus`].
//!
//! The census is conservative in one direction only: whatever it cannot
//! attribute to a static key it records as a dynamic write. A dynamic write or
//! a prototype relink withdraws nothing: it sets the shadow bit of the object
//! it writes at run time (`perry-runtime/src/object/shadow.rs`, §3.6), and
//! every binding site then tests the bit on the receiver and on the holders of
//! its static chain. Only dynamic code withdraws every constant. Precisely:
//!
//! * [`ProtoCensus::written`]: every property name written by a static-key
//!   write that is not a creation — `o.m = v`, `o["m"] = v`, compound and
//!   update assignments, `delete o.m`, destructuring assignment targets,
//!   `Object.defineProperty(o, "m", …)`, `Reflect.set/defineProperty/
//!   deleteProperty(o, "m", …)`, `__defineGetter__/__defineSetter__("m")`, and
//!   the keys of a literal source of `Object.assign`/`defineProperties`. A class
//!   body or object literal *creates* its keys; it is not a write here.
//! * [`ProtoCensus::dynamic_write`]: a write whose key is not static (`o[k] =
//!   v`, `delete o[k]`, `Object.assign(o, src)` with a non-literal source,
//!   `defineProperty(o, k, …)`): binding sites test the shadow bit.
//! * [`ProtoCensus::dynamic_code`]: `eval`, `Function(…)`, `new Function(…)`
//!   (their code writes static names the census never sees) and
//!   `Reflect.construct` with a `newTarget` (an instance of one class born
//!   with another's prototype): no name is constant.
//! * [`ProtoCensus::relink`]: a [[Prototype]] change after creation:
//!   `Object.setPrototypeOf`, `Reflect.setPrototypeOf`, a `__proto__` write,
//!   `util.inherits`/`inherits`, and `Reflect.construct` with a `newTarget`
//!   (the new object's prototype then comes from another constructor).
//!
//! A method name is constant ([`ProtoCensus::method_is_constant`]) when the
//! program has no dynamic code, no static write of that name and no instance
//! field of that name ([`ProtoCensus::fields`]: a class body's DefineField
//! creates the key on every instance, so a field never needs the runtime's
//! shadow admission). Then only a
//! dynamic write or a relink can give an object an own property of that name,
//! change a prototype's slot of it or change a chain, and each of those sets
//! the written object's shadow bit; with the bits clear, a class instance
//! resolves the name to the body its class chain declared.
//!
//! Object model step 1 adds what a key add needs to know about the
//! prototypes it could meet ([`ProtoCensus::create_add_free`]): which names
//! a write can turn into an accessor or a non-writable property
//! ([`ProtoCensus::defined`]: the static key of a `defineProperty`,
//! `defineProperties` literal source, `Reflect.defineProperty`,
//! `__defineGetter__`/`__defineSetter__`), whether such a definition has a
//! key or descriptor source the scan cannot name
//! ([`ProtoCensus::dynamic_define`]), and whether an integrity operation
//! (`freeze`, `seal`, `preventExtensions`) exists anywhere
//! ([`ProtoCensus::integrity`]), which makes every existing key of its target
//! non-writable or non-addable. `Object.create(p, props)` defines only on the
//! object it creates, so its descriptor bag is none of these.
//!
//! [`ProtoCensus::shadow_names`] are the constant class member names: the
//! runtime sets the bit when one of them is added to, deleted from or
//! redefined on an object (the entry module emits them, section
//! `perry_shadow_names`).

use std::collections::BTreeSet;

use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

use super::{object_literal_keys, peel, static_member_name, str_arg};

/// Property names the runtime itself writes onto program objects through an
/// API call the scan sees only as a call (`Error.captureStackTrace(o)`, an
/// EventEmitter or stream base constructor applied to `this`). A class method
/// of one of these names is never treated as constant.
const RUNTIME_WRITTEN_NAMES: &[&str] = &[
    "stack",
    "message",
    "name",
    "cause",
    "code",
    "errno",
    "syscall",
    "path",
    "_events",
    "_eventsCount",
    "_maxListeners",
    "domain",
    "_readableState",
    "_writableState",
    "readable",
    "writable",
    "allowHalfOpen",
    "destroyed",
];

/// The program-wide prototype-write census.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ProtoCensus {
    /// Names written by a static-key, non-creating write anywhere.
    pub written: BTreeSet<String>,
    /// A write with a non-static key, or dynamic code, exists somewhere.
    pub dynamic_write: bool,
    /// A [[Prototype]] relink exists somewhere.
    pub relink: bool,
    /// `eval`/`Function`/`Reflect.construct` with a `newTarget` somewhere.
    pub dynamic_code: bool,
    /// Every static property name in the program (including literal holders).
    pub members: BTreeSet<String>,
    /// The prototype members among them (methods and accessors).
    pub methods: BTreeSet<String>,
    /// Instance field names a class body defines (class fields, auto
    /// accessors' backing names and TS parameter properties): own keys every
    /// instance is born with, so never a constant method name.
    pub fields: BTreeSet<String>,
    /// Names a static-key definition can make an accessor or non-writable
    /// (object model step 1).
    pub defined: BTreeSet<String>,
    /// A definition whose key or descriptor source is not static, or a
    /// definer named as a value.
    pub dynamic_define: bool,
    /// `freeze`/`seal`/`preventExtensions` called or named somewhere.
    pub integrity: bool,
    /// Modules scanned (diagnostics only).
    pub modules: usize,
}

impl ProtoCensus {
    /// Is every prototype slot named `name` written exactly once (at its
    /// creation), with no own property of that name ever added later?
    pub fn method_is_constant(&self, name: &str) -> bool {
        !self.dynamic_code
            && !self.written.contains(name)
            && !self.fields.contains(name)
            && !RUNTIME_WRITTEN_NAMES.contains(&name)
    }

    /// Object model step 1 (§3.2): can no `[[Set]]` of `name` on an object
    /// whose [[Prototype]] chain is a closed object literal (data keys only,
    /// born writable) and then `%Object.prototype%` be intercepted? Then a
    /// key add of `name` at that object's creation point is a slot store.
    ///
    /// Intercepting needs an accessor or a non-writable property of `name`
    /// on the chain, or a different chain. A closed literal is born with
    /// neither, and `%Object.prototype%`'s only accessor is `__proto__`; so
    /// only a definition of `name` (static, or one the scan cannot name), an
    /// integrity operation, a relink, or dynamic code can make one.
    pub fn create_add_free(&self, name: &str) -> bool {
        !self.dynamic_code
            && !self.relink
            && !self.dynamic_define
            && !self.integrity
            && name != "__proto__"
            && !self.defined.contains(name)
    }

    /// Can a dynamic write or a relink reach some object? Then a binding
    /// site tests the shadow bit of its receiver and holders; otherwise the
    /// binding needs no test at all.
    pub fn shadow_reachable(&self) -> bool {
        self.dynamic_write || self.relink
    }

    /// Every class member name: on a prototype, a write of any of them sets
    /// the shadow bit (a holder test then proves the class's own slots).
    pub fn prototype_names(&self) -> impl Iterator<Item = &String> + '_ {
        self.members
            .iter()
            .filter(|n| self.methods.contains(*n) || self.method_is_constant(n))
    }

    /// The names whose add to a class instance sets the shadow bit: every
    /// constant method or accessor name. Empty when nothing can reach an
    /// object dynamically. (Deletes, redefinitions, prototypes and objects
    /// with a recorded [[Prototype]] count every member name.)
    pub fn shadow_names(&self) -> impl Iterator<Item = &String> + '_ {
        self.methods
            .iter()
            .filter(move |n| self.shadow_reachable() && self.method_is_constant(n))
    }

    /// Fold another census in (the driver unions per-module scans).
    pub fn merge(&mut self, other: &ProtoCensus) {
        self.written.extend(other.written.iter().cloned());
        self.dynamic_write |= other.dynamic_write;
        self.relink |= other.relink;
        self.dynamic_code |= other.dynamic_code;
        self.members.extend(other.members.iter().cloned());
        self.methods.extend(other.methods.iter().cloned());
        self.fields.extend(other.fields.iter().cloned());
        self.defined.extend(other.defined.iter().cloned());
        self.dynamic_define |= other.dynamic_define;
        self.integrity |= other.integrity;
        self.modules += other.modules;
    }

    /// A stable text key for compile caches: every object compiled against
    /// one census must be rebuilt when the census changes.
    pub fn cache_key(&self) -> String {
        let mut key = format!(
            "shadow-desc-v4:d{}r{}c{}:",
            self.dynamic_write as u8, self.relink as u8, self.dynamic_code as u8
        );
        for name in &self.written {
            key.push_str(name);
            key.push('|');
        }
        key.push('#');
        for name in &self.fields {
            key.push_str(name);
            key.push('|');
        }
        key.push('#');
        for name in &self.members {
            key.push_str(name);
            key.push(if self.methods.contains(name) {
                '!'
            } else {
                '|'
            });
        }
        key.push_str(&format!(
            "#create-v1:d{}i{}:",
            self.dynamic_define as u8, self.integrity as u8
        ));
        for name in &self.defined {
            key.push_str(name);
            key.push('|');
        }
        key
    }

    /// One-line summary for `--verbose` diagnostics.
    pub fn summary(&self) -> String {
        format!(
            "proto census: {} modules, {} written names, {} members, dynamic_write={}, relink={}, dynamic_code={}",
            self.modules,
            self.written.len(),
            self.members.len(),
            self.dynamic_write,
            self.relink,
            self.dynamic_code
        )
    }
}

struct CensusScanner<'a> {
    out: &'a mut ProtoCensus,
}

impl CensusScanner<'_> {
    fn write(&mut self, key: Option<String>) {
        match key {
            Some(k) if k == "__proto__" => self.out.relink = true,
            Some(k) => {
                self.out.written.insert(k);
            }
            None => self.out.dynamic_write = true,
        }
    }

    /// A member expression used as a write target.
    fn member_target(&mut self, m: &ast::MemberExpr) {
        match &m.prop {
            ast::MemberProp::PrivateName(_) => {}
            ast::MemberProp::Ident(p) => self.write(Some(p.sym.to_string())),
            ast::MemberProp::Computed(c) => {
                // A well-known symbol (`o[Symbol.iterator] = f`) can never
                // equal a string method name.
                if well_known_symbol(&c.expr) {
                    return;
                }
                self.write(str_arg(&c.expr));
            }
        }
    }

    fn super_target(&mut self, s: &ast::SuperPropExpr) {
        match &s.prop {
            ast::SuperProp::Ident(p) => self.write(Some(p.sym.to_string())),
            ast::SuperProp::Computed(c) => self.write(str_arg(&c.expr)),
        }
    }

    fn simple_target(&mut self, t: &ast::SimpleAssignTarget) {
        match t {
            ast::SimpleAssignTarget::Member(m) => self.member_target(m),
            ast::SimpleAssignTarget::SuperProp(s) => self.super_target(s),
            ast::SimpleAssignTarget::Paren(p) => self.expr_target(&p.expr),
            ast::SimpleAssignTarget::OptChain(_) => self.out.dynamic_write = true,
            _ => {}
        }
    }

    fn expr_target(&mut self, e: &ast::Expr) {
        match peel(e) {
            ast::Expr::Member(m) => self.member_target(m),
            ast::Expr::SuperProp(s) => self.super_target(s),
            _ => {}
        }
    }

    fn pat(&mut self, pat: &ast::Pat) {
        match pat {
            ast::Pat::Expr(e) => self.expr_target(e),
            ast::Pat::Array(a) => {
                for p in a.elems.iter().flatten() {
                    self.pat(p);
                }
            }
            ast::Pat::Object(o) => {
                for p in &o.props {
                    match p {
                        ast::ObjectPatProp::KeyValue(kv) => self.pat(&kv.value),
                        ast::ObjectPatProp::Rest(r) => self.pat(&r.arg),
                        ast::ObjectPatProp::Assign(_) => {}
                    }
                }
            }
            ast::Pat::Assign(a) => self.pat(&a.left),
            ast::Pat::Rest(r) => self.pat(&r.arg),
            _ => {}
        }
    }

    /// Keys of each source of `Object.assign(t, …)` / `defineProperties(t, …)`.
    fn literal_sources(&mut self, sources: &[ast::ExprOrSpread]) {
        for src in sources {
            match (src.spread.is_none())
                .then(|| object_literal_keys(&src.expr))
                .flatten()
            {
                Some(keys) => {
                    for k in keys {
                        self.write(Some(k));
                    }
                }
                None => self.out.dynamic_write = true,
            }
        }
    }

    fn key_arg(&mut self, call_args: &[ast::ExprOrSpread], i: usize) {
        let key = call_args
            .get(i)
            .filter(|a| a.spread.is_none())
            .and_then(|a| str_arg(&a.expr));
        self.write(key);
    }

    /// A definition's key: written, and definable as an accessor or a
    /// non-writable property.
    fn define_key_arg(&mut self, call_args: &[ast::ExprOrSpread], i: usize) {
        let key = call_args
            .get(i)
            .filter(|a| a.spread.is_none())
            .and_then(|a| str_arg(&a.expr));
        match &key {
            Some(k) => {
                self.out.defined.insert(k.clone());
            }
            None => self.out.dynamic_define = true,
        }
        self.write(key);
    }

    /// The literal descriptor sources of `defineProperties(t, …)`.
    fn define_sources(&mut self, sources: &[ast::ExprOrSpread]) {
        for src in sources {
            match (src.spread.is_none())
                .then(|| object_literal_keys(&src.expr))
                .flatten()
            {
                Some(keys) => self.out.defined.extend(keys),
                None => self.out.dynamic_define = true,
            }
        }
        self.literal_sources(sources);
    }
}

fn well_known_symbol(e: &ast::Expr) -> bool {
    let ast::Expr::Member(m) = peel(e) else {
        return false;
    };
    matches!(peel(&m.obj), ast::Expr::Ident(id) if id.sym.as_ref() == "Symbol")
        && matches!(m.prop, ast::MemberProp::Ident(_))
}

/// A static class member key's name.
fn prop_name_static(k: &ast::PropName) -> Option<String> {
    match k {
        ast::PropName::Ident(i) => Some(i.sym.to_string()),
        ast::PropName::Str(s) => s.value.as_str().map(str::to_string),
        ast::PropName::Num(n)
            if n.value.fract() == 0.0 && n.value.abs() < 9_007_199_254_740_992.0 =>
        {
            Some(format!("{}", n.value as i64))
        }
        ast::PropName::Num(n) => Some(n.value.to_string()),
        ast::PropName::Computed(c) => str_arg(&c.expr),
        _ => None,
    }
}

fn callee_ident(e: &ast::Expr) -> Option<&str> {
    match peel(e) {
        ast::Expr::Ident(id) => Some(id.sym.as_ref()),
        _ => None,
    }
}

impl Visit for CensusScanner<'_> {
    fn visit_assign_expr(&mut self, a: &ast::AssignExpr) {
        match &a.left {
            ast::AssignTarget::Simple(t) => self.simple_target(t),
            ast::AssignTarget::Pat(p) => match p {
                ast::AssignTargetPat::Array(arr) => {
                    for e in arr.elems.iter().flatten() {
                        self.pat(e);
                    }
                }
                ast::AssignTargetPat::Object(o) => {
                    for p in &o.props {
                        match p {
                            ast::ObjectPatProp::KeyValue(kv) => self.pat(&kv.value),
                            ast::ObjectPatProp::Rest(r) => self.pat(&r.arg),
                            ast::ObjectPatProp::Assign(_) => {}
                        }
                    }
                }
                ast::AssignTargetPat::Invalid(_) => self.out.dynamic_write = true,
            },
        }
        a.visit_children_with(self);
    }

    fn visit_update_expr(&mut self, u: &ast::UpdateExpr) {
        self.expr_target(&u.arg);
        u.visit_children_with(self);
    }

    fn visit_unary_expr(&mut self, u: &ast::UnaryExpr) {
        if u.op == ast::UnaryOp::Delete {
            self.expr_target(&u.arg);
        }
        u.visit_children_with(self);
    }

    fn visit_for_in_stmt(&mut self, s: &ast::ForInStmt) {
        if let ast::ForHead::Pat(p) = &s.left {
            self.pat(p);
        }
        s.visit_children_with(self);
    }

    fn visit_for_of_stmt(&mut self, s: &ast::ForOfStmt) {
        if let ast::ForHead::Pat(p) = &s.left {
            self.pat(p);
        }
        s.visit_children_with(self);
    }

    fn visit_new_expr(&mut self, n: &ast::NewExpr) {
        if callee_ident(&n.callee) == Some("Function") {
            self.out.dynamic_code = true;
        }
        n.visit_children_with(self);
    }

    /// A mutator named as a VALUE (`var __defProp = Object.defineProperty`,
    /// `const sp = Object.setPrototypeOf`, `Object.assign.apply(…)`) can be
    /// called with any key on any object: the scan cannot follow it. Direct
    /// calls are classified in [`Self::visit_call_expr`], which does not visit
    /// the callee member itself.
    fn visit_member_expr(&mut self, m: &ast::MemberExpr) {
        if let Some(name) = static_member_name(&m.prop) {
            self.out.members.insert(name);
        }
        if let (Some(owner), Some(name)) = (callee_ident(&m.obj), static_member_name(&m.prop)) {
            match (owner, name.as_str()) {
                ("Object" | "Reflect", "setPrototypeOf") => self.out.relink = true,
                ("Reflect", "construct") => self.out.dynamic_code = true,
                ("Object", "defineProperty" | "defineProperties" | "assign")
                | ("Reflect", "defineProperty" | "set" | "deleteProperty") => {
                    self.out.dynamic_write = true;
                    if name.starts_with("define") {
                        self.out.dynamic_define = true;
                    }
                }
                ("Object", "freeze" | "seal" | "preventExtensions")
                | ("Reflect", "preventExtensions") => self.out.integrity = true,
                _ => {}
            }
        }
        if matches!(
            static_member_name(&m.prop).as_deref(),
            Some("__defineGetter__" | "__defineSetter__" | "inherits")
        ) {
            self.out.dynamic_write = true;
            self.out.dynamic_define = true;
            self.out.relink = true;
        }
        // `Function.prototype…` / `eval.x` name the value without calling or
        // aliasing it: only the property is visited.
        if matches!(callee_ident(&m.obj), Some("Function" | "eval")) {
            if let ast::MemberProp::Computed(c) = &m.prop {
                c.visit_with(self);
            }
            return;
        }
        m.visit_children_with(self);
    }

    /// `x instanceof Function` reads the constructor without calling it.
    fn visit_bin_expr(&mut self, b: &ast::BinExpr) {
        if b.op == ast::BinaryOp::InstanceOf && callee_ident(&b.right) == Some("Function") {
            b.left.visit_with(self);
            return;
        }
        b.visit_children_with(self);
    }

    fn visit_object_lit(&mut self, obj: &ast::ObjectLit) {
        for prop in &obj.props {
            let ast::PropOrSpread::Prop(prop) = prop else {
                continue;
            };
            let name = match prop.as_ref() {
                ast::Prop::Shorthand(p) => Some(p.sym.to_string()),
                ast::Prop::KeyValue(p) => prop_name_static(&p.key),
                ast::Prop::Method(p) => prop_name_static(&p.key),
                ast::Prop::Getter(p) => prop_name_static(&p.key),
                ast::Prop::Setter(p) => prop_name_static(&p.key),
                ast::Prop::Assign(p) => Some(p.key.sym.to_string()),
            };
            if let Some(name) = name {
                self.out.members.insert(name);
            }
        }
        obj.visit_children_with(self);
    }

    /// Class members: the names a class creates (never writes).
    fn visit_class(&mut self, c: &ast::Class) {
        for member in &c.body {
            if let ast::ClassMember::Method(m) = member {
                if !m.is_static {
                    if let Some(k) = prop_name_static(&m.key) {
                        self.out.methods.insert(k);
                    }
                }
            }
            let key = match member {
                ast::ClassMember::Method(m) => prop_name_static(&m.key),
                ast::ClassMember::ClassProp(p) => prop_name_static(&p.key),
                ast::ClassMember::AutoAccessor(a) => match &a.key {
                    ast::Key::Public(k) => prop_name_static(k),
                    ast::Key::Private(_) => None,
                },
                _ => None,
            };
            if let Some(k) = &key {
                self.out.members.insert(k.clone());
            }
            match member {
                ast::ClassMember::ClassProp(p) if !p.is_static => {
                    if let Some(k) = key {
                        self.out.fields.insert(k);
                    }
                }
                ast::ClassMember::Constructor(ctor) => {
                    for param in &ctor.params {
                        if let ast::ParamOrTsParamProp::TsParamProp(p) = param {
                            let name = match &p.param {
                                ast::TsParamPropParam::Ident(i) => Some(i.id.sym.to_string()),
                                ast::TsParamPropParam::Assign(a) => match &*a.left {
                                    ast::Pat::Ident(i) => Some(i.id.sym.to_string()),
                                    _ => None,
                                },
                            };
                            if let Some(name) = name {
                                self.out.fields.insert(name);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        c.visit_children_with(self);
    }

    // Types are erased: `f: Function` names no code.
    fn visit_ts_type(&mut self, _: &ast::TsType) {}
    fn visit_ts_type_ann(&mut self, _: &ast::TsTypeAnn) {}
    fn visit_ts_type_param_instantiation(&mut self, _: &ast::TsTypeParamInstantiation) {}
    fn visit_ts_interface_decl(&mut self, _: &ast::TsInterfaceDecl) {}
    fn visit_ts_type_alias_decl(&mut self, _: &ast::TsTypeAliasDecl) {}
    fn visit_ts_module_decl(&mut self, _: &ast::TsModuleDecl) {}

    fn visit_ident(&mut self, id: &ast::Ident) {
        // `eval`/`Function` passed or aliased as a value.
        if matches!(id.sym.as_ref(), "eval" | "Function") {
            self.out.dynamic_code = true;
        }
    }

    fn visit_call_expr(&mut self, call: &ast::CallExpr) {
        // Visit the arguments, and the callee except a member callee itself
        // (its receiver is still visited): a direct call is classified below.
        call.args.visit_with(self);
        call.type_args.visit_with(self);
        match &call.callee {
            ast::Callee::Expr(e) => match peel(e) {
                ast::Expr::Member(m) => {
                    if let Some(name) = static_member_name(&m.prop) {
                        self.out.members.insert(name);
                    }
                    m.obj.visit_with(self);
                    if let ast::MemberProp::Computed(c) = &m.prop {
                        c.visit_with(self);
                    }
                }
                other => other.visit_with(self),
            },
            other => other.visit_with(self),
        }
        let ast::Callee::Expr(callee) = &call.callee else {
            return;
        };
        let args = &call.args;
        match peel(callee) {
            ast::Expr::Ident(id) => match id.sym.as_ref() {
                "eval" | "Function" => self.out.dynamic_code = true,
                "inherits" | "setPrototypeOf" => self.out.relink = true,
                _ => {}
            },
            ast::Expr::Member(m) => {
                let Some(method) = static_member_name(&m.prop) else {
                    return;
                };
                let owner = callee_ident(&m.obj);
                match (owner, method.as_str()) {
                    (Some("Object" | "Reflect"), "setPrototypeOf") | (_, "inherits") => {
                        self.out.relink = true
                    }
                    (Some("Reflect"), "construct") if args.len() >= 3 => {
                        self.out.dynamic_code = true
                    }
                    (Some("Object" | "Reflect"), "defineProperty") => self.define_key_arg(args, 1),
                    (Some("Reflect"), "set" | "deleteProperty") => self.key_arg(args, 1),
                    (Some("Object"), "defineProperties") => {
                        self.define_sources(args.get(1..).unwrap_or(&[]))
                    }
                    (Some("Object"), "assign") => {
                        self.literal_sources(args.get(1..).unwrap_or(&[]))
                    }
                    (Some("Object"), "freeze" | "seal" | "preventExtensions")
                    | (Some("Reflect"), "preventExtensions") => self.out.integrity = true,
                    (_, "__defineGetter__" | "__defineSetter__") => self.define_key_arg(args, 0),
                    (Some("Error"), "captureStackTrace") => self.write(Some("stack".into())),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

/// Fold `module`'s writes into `out`.
pub fn scan_module(module: &ast::Module, out: &mut ProtoCensus) {
    out.modules += 1;
    module.visit_with(&mut CensusScanner { out });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn census(src: &str) -> ProtoCensus {
        let mut cache = perry_diagnostics::SourceCache::new();
        let module = perry_parser::parse_typescript_with_cache(src, "test.ts", &mut cache)
            .expect("parse")
            .module;
        let mut out = ProtoCensus::default();
        scan_module(&module, &mut out);
        out
    }

    #[test]
    fn class_definitions_are_creations_not_writes() {
        let c = census("class C { m() { return 1 } } const o = { m() {} }; new C().m();");
        assert!(c.method_is_constant("m"));
    }

    #[test]
    fn instance_fields_are_never_constant_methods() {
        let c = census("class A { m() {} k() {} } class B extends A { m = 1; static k = 2 }");
        assert!(!c.method_is_constant("m"));
        assert!(c.method_is_constant("k"));
        let c = census(
            "class A { m() {} } class B extends A { constructor(public m: number) { super() } }",
        );
        assert!(!c.method_is_constant("m"));
    }

    #[test]
    fn every_write_form_withdraws_the_name() {
        for src in [
            "o.m = 1",
            "o['m'] = 1",
            "o.m += 1",
            "o.m++",
            "delete o.m",
            "({ a: o.m } = x)",
            "[o.m] = x",
            "for (o.m of xs) {}",
            "Object.defineProperty(o, 'm', {})",
            "Reflect.set(o, 'm', 1)",
            "Object.assign(o, { m() {} })",
            "o.__defineGetter__('m', f)",
        ] {
            let c = census(src);
            assert!(!c.method_is_constant("m"), "{src}");
            assert!(c.method_is_constant("other"), "{src}");
        }
    }

    #[test]
    fn dynamic_writes_and_relinks_need_the_shadow_test() {
        for src in [
            "o[k] = 1",
            "delete o[k]",
            "Object.assign(o, src)",
            "Object.defineProperty(o, k, {})",
            "Object.setPrototypeOf(o, p)",
            "o.__proto__ = p",
            "util.inherits(A, B)",
            "var __defProp = Object.defineProperty",
            "const sp = Object.setPrototypeOf; sp(a, b)",
            "Object.assign.apply(null, xs)",
        ] {
            let c = census(src);
            assert!(c.method_is_constant("m"), "{src}");
            assert!(c.shadow_reachable(), "{src}");
        }
    }

    #[test]
    fn dynamic_code_withdraws_everything() {
        for src in [
            "eval(s)",
            "new Function(s)",
            "Function(s)",
            "const e = eval",
            "Reflect.construct(A, [], B)",
        ] {
            assert!(!census(src).method_is_constant("m"), "{src}");
        }
        for src in ["x instanceof Function", "Function.prototype.call.bind(f)"] {
            assert!(census(src).method_is_constant("m"), "{src}");
        }
    }

    #[test]
    fn shadow_names_are_constant_members() {
        let c = census("class C { m() {} get g() { return 1 } f = 1; w() {} } o.w = 1; o[k] = 2;");
        let names: Vec<&str> = c.shadow_names().map(String::as_str).collect();
        assert_eq!(names, ["g", "m"]);
        assert_eq!(census("class C { m() {} }").shadow_names().count(), 0);
    }

    #[test]
    fn literal_holder_keys_are_shadow_names() {
        let c = census("const p = { pa: 1 }; const o = Object.create(p); o[k] = 2; o.pa;");
        assert!(c.prototype_names().any(|n| n == "pa"));
        assert_eq!(c.shadow_names().count(), 0);
    }

    #[test]
    fn type_names_are_not_code() {
        assert!(
            census("let f: Function; type F = Function; class C { m(g: Function) {} }")
                .method_is_constant("m")
        );
    }

    #[test]
    fn create_adds_answer_from_definitions() {
        let c = census(
            "const p = { a: 1 }; const o = Object.assign(Object.create(p), { v: 1 }); o.w = 2;",
        );
        assert!(c.create_add_free("v") && c.create_add_free("w") && c.create_add_free("a"));
        assert!(!c.create_add_free("__proto__"));
        let c = census("Object.defineProperty(p, 'v', { set(x) {} })");
        assert!(!c.create_add_free("v") && c.create_add_free("w"));
        let c = census("Object.defineProperties(p, { v: { value: 1 } })");
        assert!(!c.create_add_free("v") && c.create_add_free("w"));
        let c = census("p.__defineSetter__('v', f)");
        assert!(!c.create_add_free("v"));
        // `Object.create`'s descriptor bag defines only on the new object.
        let c = census("const q = Object.create(p, descs)");
        assert!(c.create_add_free("v"));
        for src in [
            "Object.defineProperty(p, k, d)",
            "Object.defineProperties(p, ds)",
            "const dp = Object.defineProperty",
            "Object.freeze(p)",
            "Object.seal(p)",
            "Reflect.preventExtensions(p)",
            "Object.setPrototypeOf(p, q)",
            "eval(s)",
        ] {
            assert!(!census(src).create_add_free("v"), "{src}");
        }
    }

    #[test]
    fn well_known_symbol_writes_name_no_method() {
        assert!(census("o[Symbol.iterator] = f").method_is_constant("m"));
    }
}
