//! #10421: can this module reach the `Function` constructor as a VALUE?
//!
//! The auto-optimized runtime links the `dyn-eval` interpreter only when the
//! compile records a reason to (`eval_classifier::has_deferred_dynamic_code_sites`).
//! Literal `new Function(...)` / `Function(...)` sites record it from their own
//! lowering. A constructor reached any other way never passed through that
//! lowering, so the program compiled, linked a runtime without the interpreter,
//! and threw `dynamic code generation ... is not supported` at the first call —
//! while a `PERRY_NO_AUTO_OPTIMIZE=1` build of the same program worked.
//!
//! This scan names those other ways. It is deliberately over-approximate: a
//! false positive links an interpreter the program never calls (binary size),
//! a false negative breaks the program at runtime.
//!
//! - any value use of the identifier `Function` (`const F = Function`,
//!   `Reflect.construct(Function, …)`, `module.exports = Function`,
//!   `Function.bind(…)`, `class X extends Function`), except the uses that
//!   cannot construct: `Function.prototype` / `.name` / `.length`,
//!   `typeof Function`, `x instanceof Function`, `x === Function`. The literal
//!   `Function(…)`, `Function.call(…)`, `Function.apply(…)` and
//!   `new Function(…)` callees are left to their lowering, which records the
//!   site only when the constant fold did not compile it;
//! - a property named `Function` on anything (`globalThis.Function`,
//!   lodash's `var Function = context.Function`, `{ Function: F } = …`);
//! - `globalThis[key]` / `global[key]` with a key that is not a literal;
//! - `.constructor` read on something that is statically a function (a
//!   function, arrow or class expression, `Object.getPrototypeOf` of one, or a
//!   name declared as one), and any `x.constructor(…)` call — calling a
//!   constructor without `new` only produces a value for a plain function,
//!   whose `constructor` is `Function`.
//!
//! The walk above goes by name. When it finds a reach, a second walk over the
//! module with its bindings resolved (swc `resolver`) decides it, so these are
//! no reach:
//!
//! - the bare name of a member inside its TypeScript enum's initializers
//!   (`Value = Function | Class` in `enum SymbolFlags`): that is the member;
//! - `E.Function` where `E` is the module's own enum: an enum object holds
//!   its members' values (the initializers, walked here) and nothing else;
//!   where `E` is an import binding it is no reach if the driver finds `E`
//!   bound to another module's enum
//!   (`eval_classifier::resolve_enum_member_receivers`);
//! - `x[k]` / `x.constructor` where `x` is a parameter or local that only
//!   shares its name with a function declared elsewhere in the module;
//! - `new C(..).eval(..)` / `c.eval(..)` (`const c = new C(..)`) where `C`
//!   is a sealed class of the module with a prototype method `eval` (or
//!   `Function`): the callee is that program method, neither a direct eval
//!   (the bare global `eval`, ECMA-262 IsDirectEval) nor a value that may be
//!   %eval% (see [`sealed_methods`]; chess `standins/gl_rules.ts` `Cond`).

use std::collections::{HashMap, HashSet};
use swc_common::{Globals, Mark, GLOBALS};
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitMutWith, VisitWith};

/// Record the module for the auto-optimize `dyn-eval` decision when it can
/// reach the `Function` constructor through a value.
pub(crate) fn pre_scan_function_ctor_reach(ast_module: &ast::Module, path: &str) {
    // Once any module reaches the constructor the answer cannot change.
    // (A module's deferred enum receivers are no such answer.)
    if crate::eval_classifier::function_ctor_reach_found() {
        return;
    }
    match module_reach(ast_module) {
        Reach::No => {}
        Reach::Yes => {
            crate::eval_classifier::note_dynamic_function_reachable();
            return;
        }
        Reach::ImportReceivers(names) => {
            crate::eval_classifier::defer_enum_member_receivers(path, names)
        }
    }
    // Constant `Function`/`eval` source is compiled as program code
    // (get-intrinsic's `(async function () {}).constructor`), so its value
    // uses count too.
    for source in crate::dynamic_code::constant_source_bodies(ast_module) {
        if let Ok(parsed) = perry_parser::parse_typescript(&source, "<static source>.cjs") {
            if module_reaches_function_ctor_value(&parsed) {
                crate::eval_classifier::note_dynamic_function_reachable();
                return;
            }
        }
    }
}

pub(crate) fn module_reaches_function_ctor_value(ast_module: &ast::Module) -> bool {
    let mut scan = Scan::default();
    ast_module.visit_with(&mut scan);
    scan.reached()
}

#[derive(Debug, PartialEq, Eq)]
enum Reach {
    No,
    Yes,
    /// Only `X.Function` reads on these import bindings: a reach unless each
    /// is bound to another module's enum.
    ImportReceivers(Vec<String>),
}

fn module_reach(ast_module: &ast::Module) -> Reach {
    if !module_reaches_function_ctor_value(ast_module) {
        return Reach::No;
    }
    // The walk by name found a reach: decide it with the bindings resolved.
    GLOBALS.set(&Globals::new(), || {
        let mut module = ast_module.clone();
        module.visit_mut_with(&mut swc_ecma_transforms_base::resolver(
            Mark::new(),
            Mark::new(),
            true,
        ));
        let mut scan = Scan {
            resolved: true,
            ..Scan::default()
        };
        module.visit_with(&mut scan);
        if scan.reached() {
            return Reach::Yes;
        }
        let mut names = Vec::new();
        for (c, name) in &scan.new_receivers {
            if !scan.class_method(c, name) {
                return Reach::Yes;
            }
        }
        for (r, name) in &scan.member_receivers {
            if scan.enum_ids.contains(r) || scan.instance_method(r, name) {
                continue;
            }
            if !scan.import_ids.contains(r) {
                return Reach::Yes;
            }
            let name = r.0.to_string();
            if !names.contains(&name) {
                names.push(name);
            }
        }
        if names.is_empty() {
            Reach::No
        } else {
            Reach::ImportReceivers(names)
        }
    })
}

#[derive(Default)]
struct Scan {
    found: bool,
    /// Bindings are resolved (`module_reach`): an identifier is its binding
    /// (name and syntax context), not its name.
    resolved: bool,
    /// Bindings declared as a function / class, or initialized with a
    /// function, arrow or class expression.
    function_names: HashSet<ast::Id>,
    /// Identifiers whose `.constructor` is read. Resolved against
    /// `function_names` after the walk, since declarations hoist.
    constructor_receivers: Vec<ast::Id>,
    function_aliases: Vec<(ast::Id, ast::Id)>,
    /// Member names of the enclosing TypeScript enums, innermost last.
    enum_members: Vec<HashSet<String>>,
    /// The module's enum bindings.
    enum_ids: HashSet<ast::Id>,
    /// The module's value import bindings (named and default).
    import_ids: HashSet<ast::Id>,
    /// Receivers of `X.Function` / `X.eval` (resolved walk only).
    member_receivers: Vec<(ast::Id, String)>,
    /// `new C(..).Function` / `new C(..).eval` reads: (C, member name)
    /// (resolved walk only, as the fields below).
    new_receivers: Vec<(ast::Id, String)>,
    /// References of each binding: its declaration, reads, writes, exports
    /// (types aside: they do not run).
    refs: HashMap<ast::Id, usize>,
    /// `new C(..)` callees.
    news: HashMap<ast::Id, usize>,
    /// `x.m(..)` calls: (x, m).
    method_calls: Vec<(ast::Id, String)>,
    /// `x = new C(..)` declarators: x -> C.
    instances: HashMap<ast::Id, ast::Id>,
    /// The module's sealed classes ([`sealed_methods`]) and their prototype
    /// method names.
    sealed: HashMap<ast::Id, HashSet<String>>,
    /// `export class C`.
    exported: HashSet<ast::Id>,
}

impl Scan {
    fn reached(&mut self) -> bool {
        // Aliases may precede their declaration or be assigned later. Keep
        // every possible function source; a reassignment only makes this
        // conservative.
        loop {
            let before = self.function_names.len();
            for (binding, source) in &self.function_aliases {
                if self.function_names.contains(source) {
                    self.function_names.insert(binding.clone());
                }
            }
            if self.function_names.len() == before {
                break;
            }
        }
        self.found
            || self
                .constructor_receivers
                .iter()
                .any(|id| self.function_names.contains(id))
    }

    /// `new C(..).name` reads the program method `C.prototype.name`: `C` is
    /// sealed with that method and only `new C(..)` names it (no export, no
    /// `C.prototype`, no reassignment), so nothing replaces the method.
    fn class_method(&self, c: &ast::Id, name: &str) -> bool {
        !self.exported.contains(c)
            && self.refs.get(c).copied().unwrap_or(0) == 1 + self.news.get(c).copied().unwrap_or(0)
            && self.sealed.get(c).is_some_and(|m| m.contains(name))
    }

    /// `x.name` reads the program method of the sealed class `C` when `x` is
    /// declared `x = new C(..)` and every other reference of `x` calls one of
    /// `C`'s methods (`x.m(..)`): the instance is never handed out, so
    /// nothing re-parents it or gives it an own property.
    fn instance_method(&self, x: &ast::Id, name: &str) -> bool {
        let Some(c) = self.instances.get(x) else {
            return false;
        };
        let Some(methods) = self.sealed.get(c) else {
            return false;
        };
        let calls: Vec<&str> = self
            .method_calls
            .iter()
            .filter(|(r, _)| r == x)
            .map(|(_, m)| m.as_str())
            .collect();
        self.class_method(c, name)
            && self.refs.get(x).copied().unwrap_or(0) == 1 + calls.len()
            && calls.iter().all(|m| methods.contains(*m))
    }

    /// The bare name of a member of an enclosing enum, inside its body.
    fn is_enum_member(&self, name: &str) -> bool {
        self.enum_members.last().is_some_and(|m| m.contains(name))
    }
}

fn peel(mut e: &ast::Expr) -> &ast::Expr {
    loop {
        match e {
            ast::Expr::Paren(p) => e = &p.expr,
            ast::Expr::TsAs(x) => e = &x.expr,
            ast::Expr::TsTypeAssertion(x) => e = &x.expr,
            ast::Expr::TsNonNull(x) => e = &x.expr,
            ast::Expr::TsConstAssertion(x) => e = &x.expr,
            ast::Expr::TsSatisfies(x) => e = &x.expr,
            _ => return e,
        }
    }
}

fn is_ident(e: &ast::Expr, name: &str) -> bool {
    matches!(peel(e), ast::Expr::Ident(id) if id.sym.as_ref() == name)
}

/// The static name of a member property: `.name` or `["name"]`.
fn member_prop_name(m: &ast::MemberExpr) -> Option<&str> {
    match &m.prop {
        ast::MemberProp::Ident(id) => Some(id.sym.as_ref()),
        ast::MemberProp::Computed(c) => match peel(&c.expr) {
            ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str(),
            _ => None,
        },
        ast::MemberProp::PrivateName(_) => None,
    }
}

/// `recv` when `e` is a plain `recv.constructor` read.
fn constructor_read_receiver(e: &ast::Expr) -> Option<&ast::Expr> {
    match peel(e) {
        ast::Expr::Member(m) if member_prop_name(m) == Some("constructor") => Some(m.obj.as_ref()),
        _ => None,
    }
}

fn is_function_like(e: &ast::Expr) -> bool {
    match peel(e) {
        ast::Expr::Fn(_) | ast::Expr::Arrow(_) | ast::Expr::Class(_) => true,
        // `Function.constructor`, `Function.prototype.constructor`.
        ast::Expr::Ident(id) => id.sym.as_ref() == "Function",
        ast::Expr::Member(m) => match member_prop_name(m) {
            Some("prototype") => is_ident(&m.obj, "Function"),
            Some("__proto__") => is_function_like(&m.obj),
            _ => false,
        },
        // `Object.getPrototypeOf(async function () {})` — the AsyncFunction /
        // GeneratorFunction constructor idiom.
        ast::Expr::Call(call) => {
            let ast::Callee::Expr(callee) = &call.callee else {
                return false;
            };
            let ast::Expr::Member(m) = peel(callee) else {
                return false;
            };
            member_prop_name(m) == Some("getPrototypeOf")
                && (is_ident(&m.obj, "Object") || is_ident(&m.obj, "Reflect"))
                && call.args.first().is_some_and(|a| is_function_like(&a.expr))
        }
        _ => false,
    }
}

impl Visit for Scan {
    fn visit_expr(&mut self, expr: &ast::Expr) {
        if self.found {
            return;
        }
        if let ast::Expr::Ident(id) = expr {
            if self.resolved {
                *self.refs.entry(id.to_id()).or_default() += 1;
            }
            if matches!(id.sym.as_ref(), "Function" | "eval") && !self.is_enum_member(&id.sym) {
                self.found = true;
            }
            return;
        }
        expr.visit_children_with(self);
    }

    fn visit_call_expr(&mut self, call: &ast::CallExpr) {
        if let ast::Callee::Expr(callee) = &call.callee {
            let callee = peel(callee);
            let literal_ctor_call = is_ident(callee, "Function")
                || is_ident(callee, "eval")
                || matches!(callee, ast::Expr::Member(m)
                    if is_ident(&m.obj, "Function")
                        && matches!(member_prop_name(m), Some("call" | "apply")));
            if literal_ctor_call {
                call.args.visit_with(self);
                return;
            }
            if let ast::Expr::Member(m) = callee {
                if member_prop_name(m) == Some("constructor") && !call.args.is_empty() {
                    self.found = true;
                    return;
                }
                if let (true, ast::Expr::Ident(x), Some(name)) =
                    (self.resolved, peel(&m.obj), member_prop_name(m))
                {
                    self.method_calls.push((x.to_id(), name.to_string()));
                }
            }
        }
        call.visit_children_with(self);
    }

    fn visit_new_expr(&mut self, new_expr: &ast::NewExpr) {
        if let (true, ast::Expr::Ident(c)) = (self.resolved, peel(&new_expr.callee)) {
            *self.news.entry(c.to_id()).or_default() += 1;
        }
        if is_ident(&new_expr.callee, "Function") {
            new_expr.args.visit_with(self);
            return;
        }
        new_expr.visit_children_with(self);
    }

    fn visit_member_expr(&mut self, m: &ast::MemberExpr) {
        match member_prop_name(m) {
            Some(name @ ("Function" | "eval")) => {
                if self.resolved {
                    match peel(&m.obj) {
                        ast::Expr::Ident(id) => {
                            self.member_receivers.push((id.to_id(), name.to_string()));
                            m.obj.visit_with(self);
                            return;
                        }
                        ast::Expr::New(n) => {
                            if let ast::Expr::Ident(c) = peel(&n.callee) {
                                self.new_receivers.push((c.to_id(), name.to_string()));
                                m.obj.visit_with(self);
                                return;
                            }
                        }
                        _ => {}
                    }
                }
                self.found = true;
                return;
            }
            Some("constructor") => {
                if is_function_like(&m.obj) {
                    self.found = true;
                    return;
                }
                if let ast::Expr::Ident(id) = peel(&m.obj) {
                    self.constructor_receivers.push(id.to_id());
                }
            }
            Some("prototype" | "name" | "length") if is_ident(&m.obj, "Function") => return,
            // `x.constructor.name` only inspects the constructor.
            Some("name") => {
                if let Some(receiver) = constructor_read_receiver(&m.obj) {
                    receiver.visit_with(self);
                    return;
                }
            }
            _ => {}
        }
        if let ast::MemberProp::Computed(c) = &m.prop {
            if !matches!(peel(&c.expr), ast::Expr::Lit(_)) {
                // An unknown property of a function may be `constructor`.
                // The interpreter must be installed even when its spelling
                // is computed; the class run-once boundary also consumes this
                // existing deferred-code fact.
                if is_ident(&m.obj, "globalThis")
                    || is_ident(&m.obj, "global")
                    || is_function_like(&m.obj)
                {
                    self.found = true;
                    return;
                }
                if let ast::Expr::Ident(id) = peel(&m.obj) {
                    self.constructor_receivers.push(id.to_id());
                }
            }
        }
        m.visit_children_with(self);
    }

    fn visit_unary_expr(&mut self, u: &ast::UnaryExpr) {
        if u.op == ast::UnaryOp::TypeOf
            && (is_ident(&u.arg, "Function") || is_ident(&u.arg, "eval"))
        {
            return;
        }
        u.visit_children_with(self);
    }

    fn visit_bin_expr(&mut self, b: &ast::BinExpr) {
        if matches!(
            b.op,
            ast::BinaryOp::InstanceOf
                | ast::BinaryOp::EqEq
                | ast::BinaryOp::NotEq
                | ast::BinaryOp::EqEqEq
                | ast::BinaryOp::NotEqEq
        ) {
            for side in [&b.left, &b.right] {
                if is_ident(side, "Function") {
                    continue;
                }
                // `x.constructor === Y` compares the constructor, it does not
                // hand it on.
                match constructor_read_receiver(side) {
                    Some(receiver) => receiver.visit_with(self),
                    None => side.as_ref().visit_with(self),
                }
            }
            return;
        }
        b.visit_children_with(self);
    }

    fn visit_prop(&mut self, p: &ast::Prop) {
        if let ast::Prop::Shorthand(id) = p {
            if matches!(id.sym.as_ref(), "Function" | "eval") {
                self.found = true;
                return;
            }
        }
        p.visit_children_with(self);
    }

    fn visit_object_pat_prop(&mut self, p: &ast::ObjectPatProp) {
        let key_is_function = match p {
            ast::ObjectPatProp::KeyValue(kv) => match &kv.key {
                ast::PropName::Ident(id) => id.sym.as_ref() == "Function",
                ast::PropName::Str(s) => s.value.as_str() == Some("Function"),
                _ => false,
            },
            ast::ObjectPatProp::Assign(a) => a.key.id.sym.as_ref() == "Function",
            ast::ObjectPatProp::Rest(_) => false,
        };
        if key_is_function {
            self.found = true;
            return;
        }
        p.visit_children_with(self);
    }

    fn visit_ts_enum_decl(&mut self, e: &ast::TsEnumDecl) {
        self.enum_ids.insert(e.id.to_id());
        let members = e
            .members
            .iter()
            .filter_map(|m| match &m.id {
                ast::TsEnumMemberId::Ident(i) => Some(i.sym.to_string()),
                ast::TsEnumMemberId::Str(s) => s.value.as_str().map(str::to_string),
            })
            .collect();
        self.enum_members.push(members);
        e.visit_children_with(self);
        self.enum_members.pop();
    }

    fn visit_import_decl(&mut self, d: &ast::ImportDecl) {
        if d.type_only {
            return;
        }
        for spec in &d.specifiers {
            match spec {
                ast::ImportSpecifier::Named(n) if !n.is_type_only => {
                    self.import_ids.insert(n.local.to_id());
                }
                ast::ImportSpecifier::Default(d) => {
                    self.import_ids.insert(d.local.to_id());
                }
                _ => {}
            }
        }
    }

    fn visit_fn_decl(&mut self, f: &ast::FnDecl) {
        self.function_names.insert(f.ident.to_id());
        f.visit_children_with(self);
    }

    fn visit_class_decl(&mut self, c: &ast::ClassDecl) {
        self.function_names.insert(c.ident.to_id());
        if self.resolved {
            if let Some(methods) = sealed_methods(&c.class) {
                self.sealed.insert(c.ident.to_id(), methods);
            }
        }
        c.visit_children_with(self);
    }

    fn visit_var_declarator(&mut self, d: &ast::VarDeclarator) {
        if let (ast::Pat::Ident(binding), Some(init)) = (&d.name, &d.init) {
            if matches!(
                peel(init),
                ast::Expr::Fn(_) | ast::Expr::Arrow(_) | ast::Expr::Class(_)
            ) {
                self.function_names.insert(binding.id.to_id());
            }
            if let ast::Expr::Ident(source) = peel(init) {
                self.function_aliases
                    .push((binding.id.to_id(), source.to_id()));
            }
            if let (true, ast::Expr::New(n)) = (self.resolved, peel(init)) {
                if let ast::Expr::Ident(c) = peel(&n.callee) {
                    self.instances.insert(binding.id.to_id(), c.to_id());
                }
            }
        }
        d.visit_children_with(self);
    }

    fn visit_ident(&mut self, id: &ast::Ident) {
        if self.resolved {
            *self.refs.entry(id.to_id()).or_default() += 1;
        }
    }

    fn visit_export_decl(&mut self, d: &ast::ExportDecl) {
        if let ast::Decl::Class(c) = &d.decl {
            self.exported.insert(c.ident.to_id());
        }
        d.visit_children_with(self);
    }

    /// Types do not run.
    fn visit_ts_type(&mut self, _: &ast::TsType) {}

    fn visit_assign_expr(&mut self, a: &ast::AssignExpr) {
        if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(binding)) = &a.left {
            match peel(&a.right) {
                ast::Expr::Fn(_) | ast::Expr::Arrow(_) | ast::Expr::Class(_) => {
                    self.function_names.insert(binding.id.to_id());
                }
                ast::Expr::Ident(source) => self
                    .function_aliases
                    .push((binding.id.to_id(), source.to_id())),
                _ => {}
            }
        }
        a.visit_children_with(self);
    }
}

/// A static member key: `k` or `'k'`.
fn static_key(key: &ast::PropName) -> Option<String> {
    match key {
        ast::PropName::Ident(id) => Some(id.sym.to_string()),
        ast::PropName::Str(s) => s.value.as_str().map(str::to_string),
        _ => None,
    }
}

/// The prototype method names of a class whose instances stay its own until
/// the program hands them out:
///
/// - no superclass: `new C` creates the object, with prototype
///   `C.prototype` (a base constructor may return any object), and its
///   constructor returns no value;
/// - no computed member key, no own field named like a method (it would
///   shadow it), no auto-accessor;
/// - its body (constructor, methods, fields, static members) uses `this`
///   only as `this.k` with a static `k` other than `__proto__`,
///   `prototype`, `constructor`, calls only its own methods on it
///   (`this.m(..)`), never assigns `this.m` for a method `m`, and never puts
///   `this.k` in a destructuring target, nested class or `super` reference.
///
/// Then the code that runs on an instance before the program has it is the
/// class body, which neither re-parents it, copies onto it, nor hands it to
/// other code. The scan's other rules catch every read of the intrinsics by
/// name, so a method of the class is a program function.
fn sealed_methods(class: &ast::Class) -> Option<HashSet<String>> {
    if class.super_class.is_some() {
        return None;
    }
    let mut methods = HashSet::new();
    let mut fields = HashSet::new();
    for member in &class.body {
        match member {
            ast::ClassMember::Constructor(c) => {
                for p in &c.params {
                    if let ast::ParamOrTsParamProp::TsParamProp(tp) = p {
                        let id = match &tp.param {
                            ast::TsParamPropParam::Ident(b) => &b.id,
                            ast::TsParamPropParam::Assign(a) => match &*a.left {
                                ast::Pat::Ident(b) => &b.id,
                                _ => return None,
                            },
                        };
                        fields.insert(id.sym.to_string());
                    }
                }
                if let Some(body) = &c.body {
                    let mut r = ReturnsValue(false);
                    body.visit_with(&mut r);
                    if r.0 {
                        return None;
                    }
                }
            }
            ast::ClassMember::Method(m) => {
                let k = static_key(&m.key)?;
                if !m.is_static {
                    methods.insert(k);
                }
            }
            ast::ClassMember::ClassProp(p) => {
                let k = static_key(&p.key)?;
                if !p.is_static {
                    fields.insert(k);
                }
            }
            ast::ClassMember::PrivateMethod(_)
            | ast::ClassMember::PrivateProp(_)
            | ast::ClassMember::StaticBlock(_)
            | ast::ClassMember::TsIndexSignature(_)
            | ast::ClassMember::Empty(_) => {}
            _ => return None,
        }
    }
    if !methods.is_disjoint(&fields) {
        return None;
    }
    let mut walk = ThisUses {
        methods: &methods,
        ok: true,
        stored: HashSet::new(),
        in_target: false,
    };
    for member in &class.body {
        match member {
            ast::ClassMember::Method(m) => m.function.visit_children_with(&mut walk),
            ast::ClassMember::PrivateMethod(m) => m.function.visit_children_with(&mut walk),
            other => other.visit_children_with(&mut walk),
        }
    }
    (walk.ok && walk.stored.is_disjoint(&methods)).then_some(methods)
}

/// A `return <value>` of the function body walked (nested functions,
/// arrows and classes return their own).
struct ReturnsValue(bool);

impl Visit for ReturnsValue {
    fn visit_function(&mut self, _: &ast::Function) {}
    fn visit_arrow_expr(&mut self, _: &ast::ArrowExpr) {}
    fn visit_class(&mut self, _: &ast::Class) {}
    fn visit_return_stmt(&mut self, r: &ast::ReturnStmt) {
        if r.arg.is_some() {
            self.0 = true;
        }
    }
}

/// The `this` uses of a class body ([`sealed_methods`]). Nested
/// non-arrow functions bind their own `this`.
struct ThisUses<'a> {
    methods: &'a HashSet<String>,
    ok: bool,
    /// `this.k = v` keys.
    stored: HashSet<String>,
    /// Inside a destructuring or for-in/of target.
    in_target: bool,
}

impl ThisUses<'_> {
    /// `this.k`: the static key `k`, unless it reaches the prototype.
    fn this_key<'m>(&mut self, m: &'m ast::MemberExpr) -> Option<&'m str> {
        if !matches!(peel(&m.obj), ast::Expr::This(_)) {
            return None;
        }
        match &m.prop {
            ast::MemberProp::Ident(k)
                if !matches!(k.sym.as_ref(), "__proto__" | "prototype" | "constructor") =>
            {
                Some(k.sym.as_ref())
            }
            ast::MemberProp::PrivateName(_) => Some(""),
            _ => {
                self.ok = false;
                Some("")
            }
        }
    }
}

impl Visit for ThisUses<'_> {
    fn visit_function(&mut self, _: &ast::Function) {}
    fn visit_class(&mut self, _: &ast::Class) {
        self.ok = false;
    }
    fn visit_super_prop_expr(&mut self, _: &ast::SuperPropExpr) {
        self.ok = false;
    }
    fn visit_expr(&mut self, e: &ast::Expr) {
        if let ast::Expr::This(_) = e {
            // A bare `this`: the instance is handed to other code.
            self.ok = false;
            return;
        }
        e.visit_children_with(self);
    }
    fn visit_member_expr(&mut self, m: &ast::MemberExpr) {
        if self.this_key(m).is_some() {
            if self.in_target {
                self.ok = false;
            }
            return;
        }
        m.visit_children_with(self);
    }
    fn visit_call_expr(&mut self, c: &ast::CallExpr) {
        if let ast::Callee::Expr(callee) = &c.callee {
            if let ast::Expr::Member(m) = peel(callee) {
                if let Some(k) = self.this_key(m) {
                    if !self.methods.contains(k) {
                        self.ok = false;
                    }
                    c.args.visit_with(self);
                    return;
                }
            }
        }
        c.visit_children_with(self);
    }
    fn visit_assign_expr(&mut self, a: &ast::AssignExpr) {
        match &a.left {
            ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(m)) => {
                if let Some(k) = self.this_key(m) {
                    self.stored.insert(k.to_string());
                }
            }
            ast::AssignTarget::Pat(_) => {
                let outer = std::mem::replace(&mut self.in_target, true);
                a.left.visit_with(self);
                self.in_target = outer;
                a.right.visit_with(self);
                return;
            }
            _ => {}
        }
        a.visit_children_with(self);
    }
    fn visit_for_head(&mut self, h: &ast::ForHead) {
        let outer = std::mem::replace(&mut self.in_target, matches!(h, ast::ForHead::Pat(_)));
        h.visit_children_with(self);
        self.in_target = outer;
    }
}

#[cfg(test)]
mod tests {
    use super::module_reaches_function_ctor_value;

    fn reaches(source: &str) -> bool {
        let module = perry_parser::parse_typescript(source, "function-ctor-reach.ts")
            .unwrap_or_else(|e| panic!("fixture must parse: {e}\n{source}"));
        module_reaches_function_ctor_value(&module)
    }

    #[test]
    fn value_uses_of_the_constructor_reach_it() {
        for source in [
            "const F = Function; new F('a', 'return a');",
            "const F: any = Function; F('return 1');",
            "Reflect.construct(Function, ['return 1']);",
            "module.exports = Function;",
            "Function.bind(null, 'a')('return a');",
            "new (globalThis.Function)('return 1');",
            "new globalThis['Function']('return 1');",
            "function lodash(context: any) { var Function = context.Function; }",
            "const { Function: F } = globalThis;",
            "const { Function } = globalThis as any;",
            "const o = { Function };",
            "new (globalThis['Func' + 'tion'])('return 1');",
            "const k = 'Function'; global[k];",
            "class X extends Function {}",
            "(function () {}).constructor('return 1');",
            "const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;",
            "const G = (function* () {}).constructor;",
            "function f() {} const F = f.constructor;",
            "const g = () => 1; const F = (g as any).constructor;",
            "declare const x: any; x.constructor('return 1');",
            "const F = Function.prototype.constructor;",
            "Function.call.bind(Function);",
            "const k = ['con','structor'].join(''); (() => 0)[k]('return 1');",
            "declare const k: string; function f() {} const alias = f; alias[k];",
            "declare const k: string; const alias = f; function f() {} alias[k];",
            "declare const k: string; let alias; alias = () => 0; alias[k];",
            "(Function as any)?.bind(null);",
        ] {
            assert!(
                reaches(source),
                "must reach the Function constructor: {source}"
            );
        }
    }

    /// The common non-constructing uses must stay free: they are everywhere in
    /// npm code, and each one linking the interpreter would grow the binary.
    #[test]
    fn non_constructing_uses_do_not_reach_it() {
        for source in [
            "console.log('hello');",
            "const s = Function.prototype.toString.call(() => 1);",
            "const bind = Function.prototype.bind;",
            "const n = Function.name + Function.length;",
            "const t = typeof Function;",
            "declare const v: unknown; const ok = v instanceof Function;",
            "declare const c: unknown; const same = c === Function || Function !== c;",
            "class A { clone() { return new (this.constructor as any)(); } }",
            "declare const o: any; const C = o.constructor; const k = o.constructor.name;",
            "const f = () => 1; const same = f.constructor === Function;",
            "function g() {} const n = g.constructor.name;",
            "const n = (async function () {}).constructor.name;",
            "const g = globalThis['process'];",
            "declare const self: any, key: string; self[key];",
            "let x: Function = () => 1;",
            "const f = () => 1; f[0]; f['length'];",
            "declare const values: number[], key: number; values[key];",
            // Literal constructor sites are recorded by their own lowering.
            "new Function('a', 'return a');",
            "Function('return 1');",
            "declare const body: string; Function.apply(null, ['a', body]);",
            "declare const body: string; Function.call(null, 'a', body);",
        ] {
            assert!(
                !reaches(source),
                "must not reach the Function constructor: {source}"
            );
        }
    }

    #[test]
    fn literal_constructor_arguments_are_still_scanned() {
        assert!(reaches("new Function(Function.bind(null), 'return 1');"));
        assert!(reaches("Function('a', (globalThis as any).Function);"));
    }

    fn reach(source: &str) -> super::Reach {
        let module = perry_parser::parse_typescript(source, "function-ctor-reach.ts")
            .unwrap_or_else(|e| panic!("fixture must parse: {e}\n{source}"));
        super::module_reach(&module)
    }

    /// The resolved walk decides every reach the walk by name finds: the
    /// positives above stay reaches.
    #[test]
    fn resolved_walk_keeps_every_value_use() {
        for source in [
            "const F = Function; new F('a', 'return a');",
            "function lodash(context: any) { var Function = context.Function; }",
            "const { Function: F } = globalThis;",
            "new (globalThis.Function)('return 1');",
            "const k = 'Function'; global[k];",
            "declare const x: any; x.constructor('return 1');",
            "function f() {} const F = f.constructor;",
            "declare const k: string; function f() {} const alias = f; alias[k];",
            "declare const root: any; const F = root.Function;",
        ] {
            assert_eq!(reach(source), super::Reach::Yes, "{source}");
        }
    }

    /// tssrc (TypeScript's own sources): `SymbolFlags.Function`, the
    /// `Value = Function | Class` initializer inside `enum SymbolFlags`, and
    /// core.ts's `map[key]` on a parameter named like `function map`.
    #[test]
    fn enum_members_and_shadowing_names_are_no_reach() {
        for source in [
            "enum F { Function = 1, Class = 2, Value = Function | Class }",
            "enum F { Function = 'f' } const x = F.Function;",
            "const enum K { Function } const x = K.Function, y = K['Function'];",
            "export function map() {} function get(map: any, key: string) { return map[key]; }",
            "function map() {} function g(map: any) { return map.constructor; }",
        ] {
            assert_eq!(reach(source), super::Reach::No, "{source}");
        }
        // `function g(map)`'s receiver is the parameter, `map` here the function.
        assert_eq!(
            reach("function map() {} declare const k: string; map[k];"),
            super::Reach::Yes
        );
    }

    /// An import binding's `X.Function` is the driver's question (is `X`
    /// another module's enum?); anything that may be the global object or
    /// a context holding the constructor stays a reach.
    #[test]
    fn import_receivers_are_deferred_and_shadows_are_not() {
        assert_eq!(
            reach("import { SymbolFlags, X } from './types'; SymbolFlags.Function; X.Function; SymbolFlags.Function;"),
            super::Reach::ImportReceivers(vec!["SymbolFlags".into(), "X".into()])
        );
        for source in [
            // A parameter shadows the import / the enum.
            "import { E } from './e'; function f(E: any) { return E.Function; }",
            "enum E { Function } function f(E: any) { return E.Function; }",
            // A namespace import's member is no enum binding of this module.
            "import * as ts from './ts'; ts.SymbolFlags.Function;",
            "import * as ns from './ns'; ns.Function;",
            // A member that is not the enum's: `Function` is the global here.
            "enum E { A = [Function].length }",
            "enum E { Function = 1 } const g: any = globalThis; g.Function;",
            "import { E } from './e'; E.Function; declare const o: any; o.Function;",
        ] {
            assert_eq!(reach(source), super::Reach::Yes, "{source}");
        }
    }
    /// chess `standins/gl_rules.ts`: `new Cond(..).eval(dv)` and
    /// `cond.eval(rest)` call the program class's method: no eval, direct or
    /// indirect, so no reach.
    #[test]
    fn a_sealed_program_class_method_named_eval_is_no_reach() {
        for source in [
            "class Cond { private toks: string[] = []; private pos = 0; private d: number; \
             constructor(d: number) { this.d = d; } \
             eval(s: string): number { this.toks = []; this.pos = 0; return this.value(s) + this.d; } \
             private value(s: string): number { return s.length > 1 ? new Cond(this.d).eval(s.slice(1)) : this.pos; } } \
             function pre(d: number) { const cond = new Cond(d); return cond.eval('a') + cond.eval('b'); }",
            "class P { Function(): number { return 1; } } new P().Function();",
        ] {
            assert_eq!(reach(source), super::Reach::No, "{source}");
        }
        for source in [
            // The instance handed out, re-parented, or the method read as a value.
            "class C { eval() {} } const c = new C(); Object.setPrototypeOf(c, globalThis); c.eval('1');",
            "class C { eval() {} } const c = new C(); const f = c.eval; f('1');",
            "class C { eval() {} } let c = new C(); c = globalThis as any; c.eval('1');",
            "class C { eval() {} run() { Object.setPrototypeOf(this, globalThis); } } const c = new C(); c.run(); c.eval('1');",
            "class C { eval() {} other() { (this as any).__proto__ = globalThis; } } new C().eval('1');",
            "class C { constructor() { Object.setPrototypeOf(this, globalThis); } eval() {} } new C().eval('1');",
            "class C { constructor() { return globalThis; } eval() {} } new C().eval('1');",
            "class C { eval() {} f: any = 0; run() { this.f(); } } const c = new C(); c.run(); c.eval('1');",
            "class C { eval() {} run() { this.eval = (globalThis as any).x; } } new C().eval('1');",
            // No such method: the chain goes on (Object.prototype, a base).
            "class C { run() {} } new C().eval('1');",
            "class B {} class C extends B { eval() {} } new C().eval('1');",
            // The class handed out: its prototype may get any method.
            "class C { eval() {} } Object.defineProperties(C.prototype, Object.getOwnPropertyDescriptors(globalThis)); new C().eval('1');",
            "export class C { eval() {} } new C().eval('1');",
            "class C { eval() {} } export { C }; new C().eval('1');",
            // A receiver that is no instance at all.
            "declare const o: any; o.eval('1');",
        ] {
            assert_eq!(reach(source), super::Reach::Yes, "{source}");
        }
    }
}
