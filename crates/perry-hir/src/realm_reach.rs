//! Whole-program realm reachability (DESIGN §2.2). This analysis never controls
//! emission in WRT3a. Keep the five top boundaries explicit and independently
//! testable; an unfamiliar operation is not a reason to select the full realm.
use std::collections::{BTreeMap, BTreeSet};

use crate::realm_manifest::RealmEntry;
use swc_common::{Globals, Mark, SyntaxContext, GLOBALS};
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitMut, VisitMutWith, VisitWith};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RealmTop {
    DynamicCode,
    GlobalObjectValue,
    With,
    DynamicModule,
    GlobalAnyMember,
}

impl RealmTop {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::DynamicCode => "DynamicCodeReach.globals",
            Self::GlobalObjectValue => "global object used as a value",
            Self::With => "with",
            Self::DynamicModule => "non-constant require/import",
            Self::GlobalAnyMember => "PatchedBuiltins global ANY_MEMBER",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RealmReach {
    pub names: BTreeSet<String>,
    pub top: BTreeSet<RealmTop>,
}

impl RealmReach {
    pub fn is_full(&self) -> bool {
        !self.top.is_empty()
    }

    pub fn merge(&mut self, other: &Self) {
        self.names.extend(other.names.iter().cloned());
        self.top.extend(other.top.iter().cloned());
    }

    /// Apply the exact program-level signals, including patches discovered in
    /// modules other than the one currently being scanned.
    pub fn program_signals(&mut self, patched: &crate::PatchedBuiltins) {
        if patched.dynamic_code_reach.globals {
            self.top.insert(RealmTop::DynamicCode);
        }
        if patched
            .iter()
            .any(|(owner, key)| is_global(owner) && key == crate::patched_builtins::ANY_MEMBER)
        {
            self.top.insert(RealmTop::GlobalAnyMember);
        }
    }

    /// Test-only (the realm-diff gate's `PERRY_REALM_REACH_NAMES` input):
    /// R becomes exactly `names`, with every top reason dropped, so a program
    /// that reflects over `globalThis` (the realm dumper) can be built with a
    /// partial image. A top program's own names are not kept: a scan closes a
    /// top R to the whole catalog, so they are no longer distinguishable.
    /// [`close`](Self::close) then applies the birth relation as usual.
    pub fn restrict_to(&mut self, names: impl IntoIterator<Item = String>) {
        self.top.clear();
        self.names = names.into_iter().collect();
    }

    pub fn close(&mut self, entries: &[RealmEntry]) {
        if self.is_full() {
            self.names
                .extend(entries.iter().map(|e| e.name.to_string()));
            return;
        }
        let by_name: BTreeMap<_, _> = entries.iter().map(|e| (e.name, e)).collect();
        // Every program has the global cell, born on the layout its declaration
        // names (object/global_this/fetch_globals.rs): that `%Layout:` birth is
        // in every R. Its member births stay conditional (RealmPlan keeps only
        // the reached members), so `%Global%` itself is not a root.
        if let Some(global) = by_name.get("%Global%") {
            self.names.extend(
                global
                    .births
                    .iter()
                    .filter(|birth| birth.starts_with("%Layout:"))
                    .map(|birth| (*birth).to_string()),
            );
        }
        let mut pending: Vec<_> = self.names.iter().cloned().collect();
        while let Some(name) = pending.pop() {
            if let Some(entry) = by_name.get(name.as_str()) {
                for birth in entry.births {
                    if self.names.insert((*birth).to_string()) {
                        pending.push((*birth).to_string());
                    }
                }
            }
        }
    }
}

pub fn is_global(name: &str) -> bool {
    matches!(name, "globalThis" | "global" | "self" | "window" | "frames")
}

/// Scan a module's value expressions with SWC's binding resolver. A parameter
/// or local called Map/globalThis/require must not name a builtin or select top.
/// Constant evaluator bodies are ordinary program source and are scanned too.
pub fn scan(module: &ast::Module, entries: &[RealmEntry]) -> RealmReach {
    let mut out = RealmReach::default();
    let mut dynamic = crate::dynamic_code::DynamicCodeReach::default();
    crate::dynamic_code::scan(module, "<realm>", &mut dynamic);
    if dynamic.globals {
        out.top.insert(RealmTop::DynamicCode);
    }
    for name in [
        "Object",
        "Function",
        "Array",
        "Error",
        "TypeError",
        "RangeError",
        "SyntaxError",
        "ReferenceError",
        "EvalError",
        "URIError",
        "AggregateError",
        "SuppressedError",
    ] {
        if entries.iter().any(|e| e.name == name) {
            out.names.insert(name.into());
        }
    }
    scan_resolved(module, entries, &mut out);
    for source in crate::dynamic_code::constant_source_bodies(module) {
        if let Ok(parsed) = perry_parser::parse_typescript(&source, "<realm-static>.cjs") {
            // The dynamic-code scan already recursively checks constant text.
            let inner = scan(&parsed, entries);
            out.merge(&inner);
        }
    }
    out.close(entries);
    out
}

fn scan_resolved(module: &ast::Module, entries: &[RealmEntry], out: &mut RealmReach) {
    // Transforms may supply an AST resolved under a different Globals arena.
    // This analysis owns its binding identities and must not reuse those ids.
    struct Clear;
    impl VisitMut for Clear {
        fn visit_mut_syntax_context(&mut self, context: &mut SyntaxContext) {
            *context = SyntaxContext::empty();
        }
    }
    // An ambient declaration that HIR lowering erases binds nothing: references
    // to its names resolve exactly as if the line were absent (`declare
    // function require` keeps `require` the loader; `declare var x` reads the
    // global). The resolver must not see it as a binding.
    struct Ambient;
    fn erased(decl: &ast::Decl) -> bool {
        match decl {
            ast::Decl::Fn(f) => crate::lower::ambient::fn_decl_types_the_loader(f),
            ast::Decl::Var(var) => var.declare && var.decls.is_empty(),
            _ => false,
        }
    }
    impl VisitMut for Ambient {
        fn visit_mut_var_decl(&mut self, var: &mut ast::VarDecl) {
            let erased: Vec<bool> = var
                .decls
                .iter()
                .map(|d| crate::lower::ambient::declarator_binds_nothing(var, d))
                .collect();
            let mut erased = erased.into_iter();
            var.decls.retain(|_| !erased.next().unwrap_or(false));
            var.visit_mut_children_with(self);
        }
        fn visit_mut_module_items(&mut self, items: &mut Vec<ast::ModuleItem>) {
            items.visit_mut_children_with(self);
            items.retain(|item| match item {
                ast::ModuleItem::Stmt(ast::Stmt::Decl(decl)) => !erased(decl),
                ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => {
                    !erased(&export.decl)
                }
                _ => true,
            });
        }
        fn visit_mut_stmts(&mut self, stmts: &mut Vec<ast::Stmt>) {
            stmts.visit_mut_children_with(self);
            stmts.retain(|stmt| !matches!(stmt, ast::Stmt::Decl(decl) if erased(decl)));
        }
    }
    let mut resolved = module.clone();
    resolved.visit_mut_with(&mut Clear);
    resolved.visit_mut_with(&mut Ambient);
    GLOBALS.set(&Globals::new(), || {
        let unresolved = Mark::new();
        resolved.visit_mut_with(&mut swc_ecma_transforms_base::resolver(
            unresolved,
            Mark::new(),
            true,
        ));
        let mut constants = Constants::default();
        loop {
            let before = constants.0.len();
            resolved.visit_with(&mut constants);
            if constants.0.len() == before {
                break;
            }
        }
        let mut loaders = Loaders {
            unresolved,
            constants: &constants.0,
            names: BTreeSet::new(),
            makers: BTreeSet::new(),
            bound_makers: BTreeSet::new(),
        };
        loop {
            let before = (loaders.names.len(), loaders.makers.len());
            resolved.visit_with(&mut loaders);
            if (loaders.names.len(), loaders.makers.len()) == before {
                break;
            }
        }
        resolved.visit_with(&mut Scan {
            entries,
            out,
            unresolved,
            constants: &constants.0,
            loaders: &loaders,
            called_makers: BTreeSet::new(),
        });
    });
}

#[derive(Default)]
struct Constants(BTreeMap<ast::Id, String>);

fn constant_string(expr: &ast::Expr, constants: &BTreeMap<ast::Id, String>) -> Option<String> {
    let expr = crate::patched_builtins::peel(expr);
    if let ast::Expr::Ident(i) = expr {
        return constants.get(&i.to_id()).cloned();
    }
    if let ast::Expr::Bin(bin) = expr {
        if bin.op == ast::BinaryOp::Add {
            return Some(
                constant_string(&bin.left, constants)? + &constant_string(&bin.right, constants)?,
            );
        }
    }
    crate::eval_classifier::const_string_of(expr)
}

impl Visit for Constants {
    fn visit_var_decl(&mut self, decl: &ast::VarDecl) {
        if decl.kind == ast::VarDeclKind::Const {
            for binding in &decl.decls {
                if let (ast::Pat::Ident(i), Some(init)) = (&binding.name, &binding.init) {
                    if let Some(value) = constant_string(init, &self.0) {
                        self.0.insert(i.id.to_id(), value);
                    }
                }
            }
        }
        decl.visit_children_with(self);
    }
}

/// Lowering can expose the global object without a literal global spelling,
/// notably constant Function('return this')() and global-script top-level this.
/// Static member receivers still name only that member; they do not escape.
pub fn scan_lowered(module: &crate::Module, entries: &[RealmEntry], out: &mut RealmReach) {
    use crate::Expr;
    let mut static_receivers = BTreeSet::new();
    let mut permit = |receiver: &Expr, key: &str| {
        if matches!(receiver, Expr::GlobalThisExpr) {
            static_receivers.insert(receiver as *const Expr as usize);
            if entries.iter().any(|e| e.name == key) {
                out.names.insert(key.into());
            }
        }
    };
    crate::for_each_module_expr(module, &mut |expr| match expr {
        Expr::PropertyGet {
            object, property, ..
        }
        | Expr::PropertySet {
            object, property, ..
        }
        | Expr::PropertyUpdate {
            object, property, ..
        } => permit(object, property),
        Expr::IndexGet { object, index, .. }
        | Expr::IndexSet { object, index, .. }
        | Expr::IndexUpdate { object, index, .. } => {
            if let Expr::String(key) = index.as_ref() {
                permit(object, key);
            }
        }
        Expr::PutValueSet {
            target,
            key,
            receiver,
            ..
        } => {
            if let Expr::String(key) = key.as_ref() {
                permit(target, key);
                permit(receiver, key);
            }
        }
        _ => {}
    });
    crate::for_each_module_expr(module, &mut |expr| {
        if matches!(expr, Expr::GlobalThisExpr)
            && !static_receivers.contains(&(expr as *const Expr as usize))
        {
            out.top.insert(RealmTop::GlobalObjectValue);
        }
    });
    out.close(entries);
}

/// The module loaders a program can name. A loader is a function that loads
/// the module its first argument names: the free `require`, a const alias of
/// one, a `createRequire(...)` result and `getBuiltinModule` (node:process).
/// A maker is node:module's `createRequire`, whose result is a loader.
///
/// Both are matched by binding, never by the spelling of a local: a renamed
/// import (`import { createRequire as cr }`), a destructured binding and a
/// const alias are the same function. Members are matched by their static key
/// on any object (`m.createRequire`, `proc["getBuiltinModule"]`, optional
/// chains included), so an imported, aliased or passed-in `process` is
/// covered. Over-matching a user function of that name only adds the modules
/// its literal calls name, or top for any other call.
///
/// A call of a loader with a literal adds that module; any other call, and any
/// use of a loader or maker as a value (an argument, a stored or exported
/// value, `.call`/`.apply`/`.bind`), is top: its calls are not visible here.
struct Loaders<'a> {
    unresolved: Mark,
    constants: &'a BTreeMap<ast::Id, String>,
    names: BTreeSet<ast::Id>,
    makers: BTreeSet<ast::Id>,
    /// `createRequire(...)` calls whose result is a const loader alias, by
    /// span. Any other such call lets its loader escape unless called in place.
    bound_makers: BTreeSet<swc_common::Span>,
}

/// `o.p`, `o?.p`, `o["p"]`: the member access an expression is, if any.
fn member_view(expr: &ast::Expr) -> Option<&ast::MemberExpr> {
    match crate::patched_builtins::peel(expr) {
        ast::Expr::Member(member) => Some(member),
        ast::Expr::OptChain(chain) => match &*chain.base {
            ast::OptChainBase::Member(member) => Some(member),
            ast::OptChainBase::Call(_) => None,
        },
        _ => None,
    }
}

/// `{ key: binding }`, `{ key = d }`, `{ key: binding = d }`: the static key
/// and the bound identifier of one object-pattern property.
fn pattern_binding<'p>(
    prop: &'p ast::ObjectPatProp,
    constants: &BTreeMap<ast::Id, String>,
) -> Option<(String, &'p ast::Ident)> {
    match prop {
        ast::ObjectPatProp::Assign(assign) => Some((assign.key.sym.to_string(), &assign.key.id)),
        ast::ObjectPatProp::KeyValue(kv) => {
            let key = match &kv.key {
                ast::PropName::Ident(i) => i.sym.to_string(),
                ast::PropName::Str(s) => s.value.to_string_lossy().into_owned(),
                ast::PropName::Computed(c) => constant_string(&c.expr, constants)?,
                _ => return None,
            };
            let binding = match &*kv.value {
                ast::Pat::Ident(i) => &i.id,
                ast::Pat::Assign(assign) => match &*assign.left {
                    ast::Pat::Ident(i) => &i.id,
                    _ => return None,
                },
                _ => return None,
            };
            Some((key, binding))
        }
        ast::ObjectPatProp::Rest(_) => None,
    }
}

impl Loaders<'_> {
    fn free(&self, ident: &ast::Ident) -> bool {
        ident.ctxt.has_mark(self.unresolved)
    }

    fn key(&self, prop: &ast::MemberProp) -> Option<String> {
        match prop {
            ast::MemberProp::Ident(i) => Some(i.sym.to_string()),
            ast::MemberProp::Computed(c) => constant_string(&c.expr, self.constants),
            ast::MemberProp::PrivateName(_) => None,
        }
    }

    fn loader_ident(&self, ident: &ast::Ident) -> bool {
        (self.free(ident) && ident.sym == "require") || self.names.contains(&ident.to_id())
    }

    fn maker_ident(&self, ident: &ast::Ident) -> bool {
        ident.sym == "createRequire" || self.makers.contains(&ident.to_id())
    }

    /// A loader function value (not a call of a maker).
    fn loader(&self, expr: &ast::Expr) -> bool {
        if let ast::Expr::Ident(i) = crate::patched_builtins::peel(expr) {
            return self.loader_ident(i);
        }
        member_view(expr).is_some_and(|m| self.key(&m.prop).as_deref() == Some("getBuiltinModule"))
    }

    fn maker(&self, expr: &ast::Expr) -> bool {
        if let ast::Expr::Ident(i) = crate::patched_builtins::peel(expr) {
            return self.maker_ident(i);
        }
        member_view(expr).is_some_and(|m| self.key(&m.prop).as_deref() == Some("createRequire"))
    }

    fn makes_require(&self, callee: &ast::Callee) -> bool {
        matches!(callee, ast::Callee::Expr(callee) if self.maker(callee))
    }

    /// `createRequire(...)` (any maker, optional call included): its result
    /// is a loader. Returns the call's span.
    fn create_require_call(&self, expr: &ast::Expr) -> Option<swc_common::Span> {
        match crate::patched_builtins::peel(expr) {
            ast::Expr::Call(call) if self.makes_require(&call.callee) => Some(call.span),
            ast::Expr::OptChain(chain) => match &*chain.base {
                ast::OptChainBase::Call(call) if self.maker(&call.callee) => Some(call.span),
                _ => None,
            },
            _ => None,
        }
    }

    fn bind(&mut self, key: &str, binding: &ast::Ident) {
        match key {
            "getBuiltinModule" => {
                self.names.insert(binding.to_id());
            }
            "createRequire" => {
                self.makers.insert(binding.to_id());
            }
            _ => {}
        }
    }
}

impl Visit for Loaders<'_> {
    fn visit_import_decl(&mut self, import: &ast::ImportDecl) {
        if import.type_only {
            return;
        }
        for specifier in &import.specifiers {
            if let ast::ImportSpecifier::Named(named) = specifier {
                let imported = match &named.imported {
                    Some(ast::ModuleExportName::Ident(i)) => i.sym.to_string(),
                    Some(ast::ModuleExportName::Str(s)) => s.value.to_string_lossy().into_owned(),
                    None => named.local.sym.to_string(),
                };
                self.bind(&imported, &named.local);
            }
        }
    }

    fn visit_object_pat(&mut self, pattern: &ast::ObjectPat) {
        // A destructured `createRequire` / `getBuiltinModule` binding, from any
        // value: `const { createRequire: cr } = require("node:module")`.
        for prop in &pattern.props {
            if let Some((key, binding)) = pattern_binding(prop, self.constants) {
                self.bind(&key, binding);
            }
        }
        pattern.visit_children_with(self);
    }

    fn visit_var_decl(&mut self, declaration: &ast::VarDecl) {
        if declaration.kind == ast::VarDeclKind::Const {
            for binding in &declaration.decls {
                if let (ast::Pat::Ident(target), Some(init)) = (&binding.name, &binding.init) {
                    if self.loader(init) {
                        self.names.insert(target.id.to_id());
                    } else if self.maker(init) {
                        self.makers.insert(target.id.to_id());
                    } else if let Some(span) = self.create_require_call(init) {
                        self.bound_makers.insert(span);
                        self.names.insert(target.id.to_id());
                    }
                }
            }
        }
        declaration.visit_children_with(self);
    }
}

struct Scan<'a> {
    entries: &'a [RealmEntry],
    out: &'a mut RealmReach,
    unresolved: Mark,
    constants: &'a BTreeMap<ast::Id, String>,
    loaders: &'a Loaders<'a>,
    called_makers: BTreeSet<swc_common::Span>,
}

impl Scan<'_> {
    fn free(&self, ident: &ast::Ident) -> bool {
        ident.ctxt.has_mark(self.unresolved)
    }

    fn add(&mut self, name: &str) {
        if self.entries.iter().any(|e| e.name == name) {
            self.out.names.insert(name.into());
        }
    }

    fn global(&self, expr: &ast::Expr) -> bool {
        match crate::patched_builtins::peel(expr) {
            ast::Expr::Ident(i) => self.free(i) && is_global(&i.sym),
            ast::Expr::Member(member) if self.global(&member.obj) => {
                let key = match &member.prop {
                    ast::MemberProp::Ident(i) => Some(i.sym.to_string()),
                    ast::MemberProp::Computed(c) => self.static_key(&c.expr),
                    _ => None,
                };
                key.is_some_and(|key| is_global(&key))
            }
            _ => false,
        }
    }

    fn native_module(&mut self, name: &str) {
        let name = name.strip_prefix("node:").unwrap_or(name);
        for entry in self.entries {
            if entry.module == Some(name) {
                self.out.names.insert(entry.name.into());
            }
        }
    }

    fn loader_or_maker(&self, expr: &ast::Expr) -> bool {
        self.loaders.loader(expr) || self.loaders.maker(expr)
    }

    /// Visit the parts of a loader or maker expression in a position that
    /// does not let it escape (callee, `typeof`, a safe member, a const alias):
    /// everything but the escape check of the expression itself.
    fn loader_parts(&mut self, expr: &ast::Expr) {
        if let ast::Expr::Ident(i) = crate::patched_builtins::peel(expr) {
            if self.free(i) {
                self.add(&i.sym);
            }
        } else if let Some(member) = member_view(expr) {
            self.member(member, true);
        }
    }

    /// A call, optional call or `import()` (callee `None`).
    fn call(
        &mut self,
        span: swc_common::Span,
        callee: Option<&ast::Expr>,
        args: &[ast::ExprOrSpread],
    ) {
        let module_call = match callee {
            None => true,
            Some(callee) => {
                self.loaders.loader(callee)
                    || self
                        .loaders
                        .create_require_call(callee)
                        .is_some_and(|maker| {
                            self.called_makers.insert(maker);
                            true
                        })
            }
        };
        // A loader made by `createRequire` that is neither a const alias nor
        // called in place escapes: its calls are not visible here.
        if callee.is_some_and(|c| self.loaders.maker(c))
            && !self.loaders.bound_makers.contains(&span)
            && !self.called_makers.contains(&span)
        {
            self.out.top.insert(RealmTop::DynamicModule);
        }
        if module_call {
            if let Some(name) = args
                .first()
                .filter(|a| a.spread.is_none())
                .and_then(|a| constant_string(&a.expr, self.constants))
            {
                self.native_module(&name);
            } else {
                self.out.top.insert(RealmTop::DynamicModule);
            }
        } else {
            // Generic calls can receive every primitive kind. Literal-only
            // programs below name just the kinds present in their expressions.
            self.primitive_prototypes();
        }
        match callee {
            Some(callee) if self.loader_or_maker(callee) => self.loader_parts(callee),
            Some(callee) => callee.visit_with(self),
            None => {}
        }
        for arg in args {
            arg.visit_with(self);
        }
    }

    fn iterables(&mut self, value: &ast::Expr) {
        match crate::patched_builtins::peel(value) {
            ast::Expr::Array(_) => self.add("%ArrayIteratorPrototype%"),
            ast::Expr::Lit(ast::Lit::Str(_)) | ast::Expr::Tpl(_) => {
                self.add("%StringIteratorPrototype%")
            }
            _ => {
                // No type proof: it can iterate any iterable kind. This adds
                // their prototypes, never top and never unrelated namespaces.
                for name in [
                    "%IteratorPrototype%",
                    "%ArrayIteratorPrototype%",
                    "%StringIteratorPrototype%",
                    "%MapIteratorPrototype%",
                    "%SetIteratorPrototype%",
                    "%GeneratorPrototype%",
                ] {
                    self.add(name);
                }
            }
        }
    }

    fn primitive_prototypes(&mut self) {
        for name in ["String", "Number", "Boolean", "BigInt", "Symbol"] {
            self.add(name);
        }
    }

    fn static_key(&self, expr: &ast::Expr) -> Option<String> {
        if let Some(key) = constant_string(expr, self.constants) {
            return Some(key);
        }
        match crate::patched_builtins::peel(expr) {
            ast::Expr::Lit(ast::Lit::Num(n)) => Some(crate::js_number_to_string(n.value)),
            ast::Expr::Lit(ast::Lit::Bool(b)) => Some(b.value.to_string()),
            ast::Expr::Lit(ast::Lit::Null(_)) => Some("null".into()),
            ast::Expr::Ident(i)
                if self.free(i) && matches!(i.sym.as_ref(), "undefined" | "NaN" | "Infinity") =>
            {
                Some(i.sym.to_string())
            }
            _ => None,
        }
    }
}

impl Scan<'_> {
    fn member(&mut self, member: &ast::MemberExpr, read: bool) {
        if self.global(&member.obj) {
            let key = match &member.prop {
                ast::MemberProp::Ident(i) => Some(i.sym.to_string()),
                ast::MemberProp::Computed(c) => self.static_key(&c.expr),
                _ => None,
            };
            if let Some(key) = key {
                if read && is_global(&key) {
                    self.out.top.insert(RealmTop::GlobalObjectValue);
                } else {
                    self.add(&key);
                }
            } else {
                self.out.top.insert(RealmTop::GlobalObjectValue);
            }
            // A static member does not use the global object as a value.
            if let ast::MemberProp::Computed(c) = &member.prop {
                c.expr.visit_with(self);
            }
        } else if self.loader_or_maker(&member.obj)
            && self
                .loaders
                .key(&member.prop)
                .is_some_and(|key| !matches!(key.as_str(), "call" | "apply" | "bind"))
        {
            // `require.resolve(...)`, `require.main`, `require.cache`: a
            // static member other than call/apply/bind does not let the loader
            // escape.
            self.primitive_prototypes();
            self.loader_parts(&member.obj);
        } else {
            if !matches!(
                crate::patched_builtins::peel(&member.obj),
                ast::Expr::Lit(_) | ast::Expr::Array(_) | ast::Expr::Object(_) | ast::Expr::New(_)
            ) {
                self.primitive_prototypes();
            }
            member.visit_children_with(self);
        }
    }
}

impl Visit for Scan<'_> {
    fn visit_ts_type(&mut self, _: &ast::TsType) {}

    fn visit_expr(&mut self, expr: &ast::Expr) {
        // Every position that does not let a loader escape visits it through
        // `loader_parts`; reaching it here means it is used as a value.
        if self.loader_or_maker(expr) {
            self.out.top.insert(RealmTop::DynamicModule);
        }
        if let ast::Expr::Ident(i) = expr {
            if self.free(i) {
                if is_global(&i.sym) {
                    self.out.top.insert(RealmTop::GlobalObjectValue);
                } else {
                    self.add(&i.sym);
                }
            }
        }
        expr.visit_children_with(self);
    }

    fn visit_member_expr(&mut self, member: &ast::MemberExpr) {
        self.member(member, true);
    }

    fn visit_simple_assign_target(&mut self, target: &ast::SimpleAssignTarget) {
        if let ast::SimpleAssignTarget::Member(member) = target {
            self.member(member, false);
        } else {
            target.visit_children_with(self);
        }
    }

    fn visit_unary_expr(&mut self, expression: &ast::UnaryExpr) {
        if expression.op == ast::UnaryOp::TypeOf && self.loader_or_maker(&expression.arg) {
            self.loader_parts(&expression.arg);
            return;
        }
        if expression.op == ast::UnaryOp::Delete {
            if let ast::Expr::Member(member) = crate::patched_builtins::peel(&expression.arg) {
                self.member(member, false);
                return;
            }
        }
        expression.visit_children_with(self);
    }

    fn visit_prop(&mut self, prop: &ast::Prop) {
        if let ast::Prop::Shorthand(i) = prop {
            if self.loaders.loader_ident(i) || self.loaders.maker_ident(i) {
                self.out.top.insert(RealmTop::DynamicModule);
            }
            if self.free(i) {
                if is_global(&i.sym) {
                    self.out.top.insert(RealmTop::GlobalObjectValue);
                } else {
                    self.add(&i.sym);
                }
            }
        }
        prop.visit_children_with(self);
    }

    fn visit_with_stmt(&mut self, stmt: &ast::WithStmt) {
        self.out.top.insert(RealmTop::With);
        stmt.visit_children_with(self);
    }

    fn visit_import_decl(&mut self, import: &ast::ImportDecl) {
        if !import.type_only {
            self.native_module(&import.src.value.to_string_lossy());
        }
        import.visit_children_with(self);
    }

    fn visit_named_export(&mut self, export: &ast::NamedExport) {
        if !export.type_only {
            if let Some(source) = &export.src {
                self.native_module(&source.value.to_string_lossy());
            } else if export.specifiers.iter().any(|s| {
                matches!(s, ast::ExportSpecifier::Named(ast::ExportNamedSpecifier {
                    orig: ast::ModuleExportName::Ident(i), ..
                }) if self.loaders.loader_ident(i) || self.loaders.maker_ident(i))
            }) {
                // An exported loader is called in another module.
                self.out.top.insert(RealmTop::DynamicModule);
            }
        }
        export.visit_children_with(self);
    }

    fn visit_export_all(&mut self, export: &ast::ExportAll) {
        if !export.type_only {
            self.native_module(&export.src.value.to_string_lossy());
        }
    }

    fn visit_export_decl(&mut self, export: &ast::ExportDecl) {
        // `export const req = createRequire(...)`: called in another module.
        if let ast::Decl::Var(declaration) = &export.decl {
            if declaration.decls.iter().any(|d| {
                matches!(&d.name, ast::Pat::Ident(i)
                    if self.loaders.names.contains(&i.id.to_id()) || self.loaders.makers.contains(&i.id.to_id()))
            }) {
                self.out.top.insert(RealmTop::DynamicModule);
            }
        }
        export.visit_children_with(self);
    }

    fn visit_call_expr(&mut self, call: &ast::CallExpr) {
        let callee = match &call.callee {
            ast::Callee::Import(_) => None,
            ast::Callee::Expr(e) => Some(&**e),
            ast::Callee::Super(_) => {
                self.primitive_prototypes();
                call.visit_children_with(self);
                return;
            }
        };
        self.call(call.span, callee, &call.args);
    }

    fn visit_opt_call(&mut self, call: &ast::OptCall) {
        self.call(call.span, Some(&call.callee), &call.args);
    }

    fn visit_lit(&mut self, lit: &ast::Lit) {
        match lit {
            ast::Lit::Regex(_) => self.add("RegExp"),
            ast::Lit::Str(_) => self.add("String"),
            ast::Lit::Num(_) => self.add("Number"),
            ast::Lit::Bool(_) => self.add("Boolean"),
            ast::Lit::BigInt(_) => self.add("BigInt"),
            _ => {}
        }
    }

    fn visit_function(&mut self, function: &ast::Function) {
        if function.is_async {
            self.add("Promise");
            self.add("%AsyncFunctionPrototype%");
        }
        if function.is_generator {
            self.add(if function.is_async {
                "%AsyncGeneratorPrototype%"
            } else {
                "%GeneratorPrototype%"
            });
            self.add(if function.is_async {
                "%AsyncGeneratorFunctionPrototype%"
            } else {
                "%GeneratorFunctionPrototype%"
            });
        }
        function.visit_children_with(self);
    }

    fn visit_arrow_expr(&mut self, arrow: &ast::ArrowExpr) {
        if arrow.is_async {
            self.add("Promise");
            self.add("%AsyncFunctionPrototype%");
        }
        arrow.visit_children_with(self);
    }

    fn visit_await_expr(&mut self, expr: &ast::AwaitExpr) {
        self.add("Promise");
        expr.visit_children_with(self);
    }

    fn visit_for_of_stmt(&mut self, stmt: &ast::ForOfStmt) {
        self.iterables(&stmt.right);
        if stmt.is_await {
            self.add("Promise");
            self.add("%AsyncIteratorPrototype%");
            self.add("%AsyncGeneratorPrototype%");
        }
        stmt.visit_children_with(self);
    }

    fn visit_expr_or_spread(&mut self, expr: &ast::ExprOrSpread) {
        if expr.spread.is_some() {
            self.iterables(&expr.expr);
        }
        expr.visit_children_with(self);
    }

    fn visit_var_declarator(&mut self, decl: &ast::VarDeclarator) {
        // A const alias of a loader or maker: its calls are scanned by binding.
        if let (ast::Pat::Ident(target), Some(init)) = (&decl.name, &decl.init) {
            let id = target.id.to_id();
            if (self.loaders.names.contains(&id) || self.loaders.makers.contains(&id))
                && self.loader_or_maker(init)
            {
                self.loader_parts(init);
                return;
            }
        }
        if matches!(decl.name, ast::Pat::Array(_)) {
            if let Some(init) = &decl.init {
                self.iterables(init);
            }
        }
        decl.visit_children_with(self);
    }

    fn visit_array_pat(&mut self, pat: &ast::ArrayPat) {
        // Parameter/assignment patterns may lack an initializer at this site.
        self.add("%IteratorPrototype%");
        self.add("%ArrayIteratorPrototype%");
        self.add("%StringIteratorPrototype%");
        self.add("%MapIteratorPrototype%");
        self.add("%SetIteratorPrototype%");
        self.add("%GeneratorPrototype%");
        pat.visit_children_with(self);
    }

    fn visit_object_pat(&mut self, pat: &ast::ObjectPat) {
        self.primitive_prototypes();
        pat.visit_children_with(self);
    }

    fn visit_tagged_tpl(&mut self, tagged: &ast::TaggedTpl) {
        tagged.tag.visit_with(self);
        // Quasis are the template object's strings, not an extra builtin.
        for expression in &tagged.tpl.exprs {
            expression.visit_with(self);
        }
    }

    fn visit_new_expr(&mut self, expression: &ast::NewExpr) {
        self.primitive_prototypes();
        expression.visit_children_with(self);
    }

    fn visit_ts_interface_decl(&mut self, _: &ast::TsInterfaceDecl) {}
    fn visit_ts_type_alias_decl(&mut self, _: &ast::TsTypeAliasDecl) {}

    fn visit_tpl(&mut self, tpl: &ast::Tpl) {
        self.add("String");
        tpl.visit_children_with(self);
    }
}

#[cfg(test)]
#[path = "realm_reach_tests.rs"]
mod tests;
