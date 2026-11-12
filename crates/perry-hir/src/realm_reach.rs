//! Whole-program realm reachability for runtime-layout emission. Keep the five
//! top boundaries explicit: an unfamiliar operation does not select the full realm.
use std::collections::{BTreeMap, BTreeSet};

use crate::realm_reach_index::{member_of_primitive, RealmIndex, PRIMITIVE_KINDS};
use swc_common::{Globals, Mark, Spanned, SyntaxContext, GLOBALS};
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitMut, VisitMutWith, VisitWith};

#[path = "realm_reach_partition.rs"]
mod partition;
pub use partition::{KeylessRead, RealmReach, RealmTop};

pub fn primitive_prototype_member(name: &str) -> bool {
    PRIMITIVE_KINDS
        .iter()
        .any(|kind| member_of_primitive(name, kind, None))
}

pub fn is_global(name: &str) -> bool {
    matches!(name, "globalThis" | "global" | "self" | "window" | "frames")
}

/// Scan a module's value expressions with SWC's binding resolver. A parameter
/// or local called Map/globalThis/require must not name a builtin or select top.
/// Constant evaluator bodies are ordinary program source and are scanned too.
pub fn scan(module: &ast::Module, entries: &RealmIndex) -> RealmReach {
    let mut out = RealmReach::default();
    let mut dynamic = crate::dynamic_code::DynamicCodeReach::default();
    crate::dynamic_code::scan(module, "<realm>", &mut dynamic);
    if dynamic.globals {
        out.top.insert(RealmTop::DynamicCode);
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

fn scan_resolved(module: &ast::Module, entries: &RealmIndex, out: &mut RealmReach) {
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
            write: false,
            direct_callee: None,
            value_callee: None,
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

/// Both source members and lowered value reads use the global's declared slots.
pub trait RealmGlobals {
    fn global_member(&self, key: &str) -> bool;
}
impl RealmGlobals for RealmIndex {
    fn global_member(&self, key: &str) -> bool {
        self.last("%Global%")
            .is_some_and(|r| r.entry().own.iter().any(|p| p.key == key && !p.symbol))
    }
}
impl RealmGlobals for &[crate::realm_manifest::RealmEntry] {
    fn global_member(&self, key: &str) -> bool {
        self.iter()
            .find(|r| r.name == "%Global%")
            .is_some_and(|r| r.own.iter().any(|p| p.key == key && !p.symbol))
    }
}

/// The operations for which codegen materializes the global to load a value.
pub fn global_value_read<'a>(
    expr: &'a crate::Expr,
    entries: &impl RealmGlobals,
) -> Option<&'a str> {
    use crate::Expr;
    match expr {
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::GlobalGet(_)) => Some(property),
        Expr::Call { callee, args, .. } if matches!(callee.as_ref(), Expr::ExternFuncRef { name, .. } if matches!(name.as_str(), "js_global_get_optional" | "js_global_get_or_throw_unresolved")) => {
            if let [Expr::String(key)] = args.as_slice() {
                Some(key)
            } else {
                None
            }
        }
        Expr::ExternFuncRef { name, .. } if entries.global_member(name) => Some(name),
        _ => None,
    }
}

/// The runtime body selected by a lowered intrinsic operation. The emitter
/// and the reach use the same symbol for these calls.
pub const fn field_get_operation() -> &'static str {
    "js_object_get_field_by_name_f64"
}

pub fn string_index_operation() -> &'static str {
    "js_string_index_get_boxed"
}

pub fn lowered_operation(expr: &crate::Expr) -> Option<&'static str> {
    use crate::Expr;
    match expr {
        Expr::ProcessStdin => Some("js_process_stdin"),
        Expr::ProcessStdout => Some("js_process_stdout"),
        Expr::ProcessStderr => Some("js_process_stderr"),
        Expr::ObjectGetOwnPropertyDescriptor(..) => Some("js_object_get_own_property_descriptor"),
        Expr::ObjectGetOwnPropertyDescriptors(..) => Some("js_object_get_own_property_descriptors"),
        Expr::UrlNew { base: Some(_), .. } => Some("js_url_new_with_base"),
        Expr::UrlNew { base: None, .. } => Some("js_url_new"),
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::String(value)
            if property != "length" && !property.parse::<usize>().ok().is_some_and(|i|
                i.to_string() == *property && i < value.encode_utf16().count())) =>
        {
            Some(field_get_operation())
        }
        Expr::IndexGet { object, .. } if matches!(object.as_ref(), Expr::String(_)) => {
            Some(string_index_operation())
        }
        _ => None,
    }
}

pub fn scan_lowered(module: &crate::Module, entries: &RealmIndex, out: &mut RealmReach) {
    use crate::Expr;
    let mut static_receivers = BTreeSet::new();
    let mut callees = BTreeSet::new();
    let mut permit = |receiver: &Expr, key: &str| {
        if matches!(receiver, Expr::GlobalThisExpr) {
            static_receivers.insert(receiver as *const Expr as usize);
            out.global_read(entries, key);
        }
    };
    crate::for_each_module_expr(module, &mut |expr| match expr {
        Expr::Call { callee, .. } => {
            callees.insert(callee.as_ref() as *const Expr as usize);
        }
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
    if module
        .classes
        .iter()
        .any(|class| !class.static_fields.is_empty())
    {
        out.exec("js_class_register_static_field");
    }
    crate::for_each_module_expr(module, &mut |expr| {
        if let Some(symbol) = lowered_operation(expr) {
            out.exec(symbol);
        }
        if !callees.contains(&(expr as *const Expr as usize)) {
            if let Some(key) = global_value_read(expr, entries) {
                out.global_read(entries, key);
            }
        }
        if let Expr::Call { callee, .. } = expr {
            if let Expr::ExternFuncRef { name, .. } = callee.as_ref() {
                if entries.bodies(name).next().is_some() {
                    out.exec(name);
                }
            }
        }
        if matches!(expr, Expr::GlobalThisExpr)
            && !static_receivers.contains(&(expr as *const Expr as usize))
        {
            out.exec("js_get_global_this");
            out.own("%Global%");
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
    entries: &'a RealmIndex,
    out: &'a mut RealmReach,
    unresolved: Mark,
    constants: &'a BTreeMap<ast::Id, String>,
    loaders: &'a Loaders<'a>,
    called_makers: BTreeSet<swc_common::Span>,
    write: bool,
    direct_callee: Option<swc_common::Span>,
    value_callee: Option<swc_common::Span>,
}

impl Scan<'_> {
    fn free(&self, ident: &ast::Ident) -> bool {
        ident.ctxt.has_mark(self.unresolved)
    }

    fn add(&mut self, name: &str) {
        if self.entries.has(name) {
            self.out.own(name);
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
        for entry in self.entries.module_rows(name) {
            self.out.own(entry.name());
        }
    }

    fn module_member(&mut self, module: &str, key: Option<&str>) {
        let module = module.strip_prefix("node:").unwrap_or(module);
        let holders: Vec<_> = self
            .entries
            .module_rows(module)
            .map(|row| row.name())
            .collect();
        for holder in holders {
            self.own_property(holder, key, false);
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
        if let Some(callee) = callee {
            self.reflected_call(callee, args);
        }
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
        } else if let Some(callee) = callee {
            self.call_events(callee, args);
        }
        match callee {
            Some(callee) if self.loader_or_maker(callee) => self.loader_parts(callee),
            Some(callee) => {
                let previous = self.direct_callee;
                self.keyed_call(callee, args);
                self.direct_callee = if args.iter().all(|a| a.spread.is_none()) {
                    member_view(callee).map(|m| m.span)
                } else {
                    None
                };
                let previous_value = self.value_callee;
                self.value_callee = Some(callee.span());
                callee.visit_with(self);
                self.value_callee = previous_value;
                self.direct_callee = previous;
            }
            None => {}
        }
        for arg in args {
            arg.visit_with(self);
        }
    }

    fn iterables(&mut self, value: &ast::Expr) {
        self.protocol(value, crate::realm_manifest::RealmProtocol::Iterate);
        self.protocol(value, crate::realm_manifest::RealmProtocol::IterNext);
        self.protocol(value, crate::realm_manifest::RealmProtocol::IterResult);
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
        for name in PRIMITIVE_KINDS {
            self.add(name);
        }
        for name in self.entries.prototype_members(&PRIMITIVE_KINDS, None) {
            self.out.own(name);
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
    fn key(&self, prop: &ast::MemberProp) -> Option<String> {
        match prop {
            ast::MemberProp::Ident(i) => Some(i.sym.to_string()),
            ast::MemberProp::Computed(c) => self.static_key(&c.expr),
            _ => None,
        }
    }

    fn holder(&self, expr: &ast::Expr) -> Option<String> {
        match crate::patched_builtins::peel(expr) {
            ast::Expr::Ident(i) if self.free(i) && self.entries.has(&i.sym) => {
                Some(i.sym.to_string())
            }
            ast::Expr::Member(m) => {
                let holder = self.holder(&m.obj)?;
                let key = self.key(&m.prop)?;
                // An accessor's callable is not the value its getter returns.
                let (_, property) = self.property(&holder, &key, true)?;
                match property.value {
                    crate::realm_manifest::RealmOwnValue::Reference(value) => Some(value.into()),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn property(
        &self,
        holder: &str,
        key: &str,
        chain: bool,
    ) -> Option<(String, crate::realm_manifest::RealmOwn)> {
        let mut holder = holder.to_owned();
        let mut seen = BTreeSet::new();
        while seen.insert(holder.clone()) {
            let row = self.entries.first(&holder)?;
            if let Some(property) = row.entry().own.iter().find(|p| !p.symbol && p.key == key) {
                return Some((holder, *property));
            }
            if !chain {
                break;
            }
            holder = row.prototype()?.to_owned();
        }
        None
    }

    fn own_property(&mut self, holder: &str, key: Option<&str>, chain: bool) {
        if let Some(key) = key {
            if let Some((_, property)) = self.property(holder, key, chain) {
                use crate::realm_manifest::RealmOwnValue;
                match property.value {
                    RealmOwnValue::Reference(value) => self.out.own(value),
                    RealmOwnValue::Accessor { get, set } => {
                        for value in [get, set].into_iter().flatten() {
                            self.out.own(value);
                        }
                    }
                    _ => {}
                }
            }
        } else {
            let mut next = Some(holder.to_owned());
            let mut seen = BTreeSet::new();
            while let Some(holder) = next {
                if !seen.insert(holder.clone()) {
                    break;
                }
                self.own_all(&holder);
                next = if chain {
                    self.entries
                        .first(&holder)
                        .and_then(|r| r.prototype())
                        .map(str::to_owned)
                } else {
                    None
                };
            }
        }
    }

    fn keyed_member(&self, member: &ast::MemberExpr) -> bool {
        if let Some(name) = self.holder(&ast::Expr::Member(member.clone())) {
            return self.entries.named(&name).any(|r| r.argument_mode("keyed"));
        }
        let Some(key) = self.key(&member.prop) else {
            return false;
        };
        // An unnamed receiver can be Object or Object.prototype. Other
        // namespaces must be named, so ordinary Map.get is not Reflect.get.
        self.holder(&member.obj).is_none()
            && [format!("Object.{key}"), format!("Object.prototype.{key}")]
                .iter()
                .any(|name| self.entries.named(name).any(|r| r.argument_mode("keyed")))
    }

    fn keyed_call(
        &mut self,
        callee: &ast::Expr,
        args: &[ast::ExprOrSpread],
    ) -> Option<swc_common::Span> {
        let member = member_view(callee)?;
        if !self.keyed_member(member) || args.iter().any(|a| a.spread.is_some()) {
            return None;
        }
        let name = self.key(&member.prop)?;
        let (target, key, chain) = match name.as_str() {
            "defineProperty" | "getOwnPropertyDescriptor" => {
                (&args.first()?.expr, args.get(1).map(|a| &*a.expr), false)
            }
            "defineProperties" | "getOwnPropertyDescriptors" => (&args.first()?.expr, None, false),
            "get" => (&args.first()?.expr, args.get(1).map(|a| &*a.expr), true),
            "__lookupGetter__" | "__lookupSetter__" => {
                (&member.obj, args.first().map(|a| &*a.expr), true)
            }
            _ => return None,
        };
        let key = key.and_then(|key| self.static_key(key));
        if let Some(holder) = self.holder(target) {
            self.own_property(&holder, key.as_deref(), chain);
            if chain {
                self.out.chain_keys.insert(key);
            }
        } else if chain {
            if let Some(key) = key {
                for value in self.entries.prototype_members(&PRIMITIVE_KINDS, Some(&key)) {
                    self.out.own(value);
                }
                self.out.receiver_keys.insert(key);
            } else {
                self.out.any_receiver_key = true;
                self.primitive_prototypes();
            }
        } else if !matches!(
            crate::patched_builtins::peel(target),
            ast::Expr::Lit(_) | ast::Expr::Object(_) | ast::Expr::Array(_)
        ) {
            if let Some(key) = &key {
                self.out.receiver_keys.insert(key.clone());
            }
            self.out.own_keys.insert(key);
        }
        Some(member.span)
    }

    fn own_all(&mut self, holder: &str) {
        let mut pending = vec![holder.to_owned()];
        let mut seen = BTreeSet::new();
        let mut keyed = false;
        while let Some(name) = pending.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            let Some(row) = self.entries.first(&name) else {
                continue;
            };
            keyed |= row.argument_mode("keyed");
            if row.argument_mode("formatted") {
                self.out.keyless_read = Some(KeylessRead::All);
            }
            for (_, _, value) in row.members() {
                self.out.own(value);
                pending.push(value.to_owned());
            }
        }
        if keyed {
            self.out.own_keys.insert(None);
            self.out.any_receiver_key = true;
            self.primitive_prototypes();
        }
    }

    fn reflect(&mut self, target: &ast::Expr, read: KeylessRead) {
        if let Some(holder) = self.holder(target) {
            self.own_all(&holder);
        } else if !matches!(
            crate::patched_builtins::peel(target),
            ast::Expr::Lit(_) | ast::Expr::Object(_) | ast::Expr::Array(_)
        ) {
            self.out.keyless_read = self.out.keyless_read.max(Some(read));
        }
    }

    fn reflected_call(&mut self, callee: &ast::Expr, args: &[ast::ExprOrSpread]) {
        let name = self.holder(callee);
        if name.as_deref().is_some_and(|name| {
            self.entries
                .named(name)
                .any(|row| row.argument_mode("formatted"))
        }) {
            for arg in args {
                self.reflect(&arg.expr, KeylessRead::All);
            }
        }
        match name.as_deref() {
            Some("Object.assign") => {
                for arg in args.iter().skip(1) {
                    self.reflect(&arg.expr, KeylessRead::Enumerable);
                }
            }
            Some("Object.keys" | "Object.values" | "Object.entries" | "JSON.stringify") => {
                if let Some(arg) = args.first() {
                    self.reflect(&arg.expr, KeylessRead::Enumerable);
                }
            }
            Some(
                "Reflect.ownKeys"
                | "Object.getOwnPropertyNames"
                | "Object.getOwnPropertySymbols"
                | "Object.getOwnPropertyDescriptors",
            ) => {
                if let Some(arg) = args.first() {
                    self.reflect(&arg.expr, KeylessRead::All);
                }
            }
            _ => {}
        }
    }
}

impl Scan<'_> {
    fn member(&mut self, member: &ast::MemberExpr, read: bool) {
        if read && self.direct_callee != Some(member.span) {
            if let Some(name) = self.holder(&ast::Expr::Member(member.clone())) {
                self.callable_value(&name);
            }
        }
        let write = std::mem::take(&mut self.write);
        if matches!(member.prop, ast::MemberProp::PrivateName(_)) {
            member.obj.visit_with(self);
            return;
        }
        if self.direct_callee != Some(member.span)
            && self
                .holder(&ast::Expr::Member(member.clone()))
                .is_some_and(|name| {
                    self.entries
                        .named(&name)
                        .any(|r| r.argument_mode("formatted"))
                })
        {
            self.out.keyless_read = Some(KeylessRead::All);
        }
        if self.keyed_member(member) && self.direct_callee != Some(member.span) {
            // The callable escapes; its future target and key are unknown.
            self.out.own_keys.insert(None);
            self.out.any_receiver_key = true;
            self.primitive_prototypes();
        }
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
                    self.out.global_read(self.entries, &key);
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
            let key = self.key(&member.prop);
            if let Some(holder) = self.holder(&member.obj) {
                self.own_property(&holder, key.as_deref(), true);
                self.out.chain_keys.insert(key.clone());
                self.add(&holder);
                // Scan nested named receivers by key, without escaping them.
                if let Some(nested) = member_view(&member.obj) {
                    self.member(nested, true);
                }
                if let ast::MemberProp::Computed(c) = &member.prop {
                    c.expr.visit_with(self);
                }
            } else {
                match key {
                    Some(key) if write => {
                        self.out.setter_keys.insert(key);
                    }
                    None if write => self.out.any_setter_key = true,
                    Some(key) => {
                        self.out.receiver_keys.insert(key);
                    }
                    None => self.out.any_receiver_key = true,
                }
                let kinds: &[&str] = match crate::patched_builtins::peel(&member.obj) {
                    ast::Expr::Lit(ast::Lit::Str(_)) => &["String"],
                    ast::Expr::Lit(ast::Lit::Num(_)) => &["Number"],
                    ast::Expr::Lit(ast::Lit::Bool(_)) => &["Boolean"],
                    ast::Expr::Lit(ast::Lit::BigInt(_)) => &["BigInt"],
                    ast::Expr::Lit(_) | ast::Expr::Array(_) | ast::Expr::Object(_) => &[],
                    _ => &PRIMITIVE_KINDS,
                };
                for kind in kinds {
                    self.add(kind);
                }
                for name in self
                    .entries
                    .prototype_members(kinds, self.key(&member.prop).as_deref())
                {
                    if !write || name.ends_with(".<set>") {
                        self.out.own(name);
                    }
                }
                if !matches!(
                    crate::patched_builtins::peel(&member.obj),
                    ast::Expr::Lit(_)
                        | ast::Expr::Array(_)
                        | ast::Expr::Object(_)
                        | ast::Expr::New(_)
                ) {
                    for kind in PRIMITIVE_KINDS {
                        self.add(kind);
                    }
                }
                member.visit_children_with(self);
            }
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
                    if matches!(i.sym.as_ref(), "module" | "exports" | "arguments") {
                        self.objects();
                    } else if !self.entries.has(&i.sym)
                        && !matches!(
                            i.sym.as_ref(),
                            "undefined" | "NaN" | "Infinity" | "__dirname" | "__filename"
                        )
                    {
                        self.out.global_read(self.entries, &i.sym);
                    }
                    self.out.global_keys.insert(i.sym.to_string());
                    self.add(&i.sym);
                    if self.value_callee != Some(expr.span()) {
                        self.callable_value(&i.sym);
                        self.own_all(&i.sym);
                    }
                }
            }
        }
        match expr {
            ast::Expr::Object(_) | ast::Expr::MetaProp(_) => self.objects(),
            ast::Expr::Array(_) => self.instance("Array"),
            ast::Expr::Ident(i) if i.sym == "arguments" => self.objects(),
            _ => {}
        }
        expr.visit_children_with(self);
    }

    fn visit_member_expr(&mut self, member: &ast::MemberExpr) {
        self.member(member, true);
    }

    fn visit_binding_ident(&mut self, ident: &ast::BindingIdent) {
        // Resolver-marked assignment patterns write the actual global object;
        // declarations and parameters carry their own binding mark.
        if self.free(&ident.id) {
            self.out.exec("js_get_global_this");
        }
        ident.visit_children_with(self);
    }

    fn visit_assign_expr(&mut self, assign: &ast::AssignExpr) {
        self.write = assign.op == ast::AssignOp::Assign;
        assign.left.visit_with(self);
        self.write = false;
        assign.right.visit_with(self);
    }

    fn visit_for_in_stmt(&mut self, stmt: &ast::ForInStmt) {
        self.reflect(&stmt.right, KeylessRead::Enumerable);
        stmt.visit_children_with(self);
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
        if matches!(prop, ast::Prop::Getter(_) | ast::Prop::Setter(_)) {
            self.functions();
        }
        if let ast::Prop::Shorthand(i) = prop {
            if self.loaders.loader_ident(i) || self.loaders.maker_ident(i) {
                self.out.top.insert(RealmTop::DynamicModule);
            }
            if self.free(i) {
                if is_global(&i.sym) {
                    self.out.top.insert(RealmTop::GlobalObjectValue);
                } else {
                    self.out.global_keys.insert(i.sym.to_string());
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
            let source = import.src.value.to_string_lossy();
            self.native_module(&source);
            for specifier in &import.specifiers {
                let key = match specifier {
                    ast::ImportSpecifier::Named(named) if !named.is_type_only => {
                        match &named.imported {
                            Some(ast::ModuleExportName::Ident(i)) => Some(i.sym.to_string()),
                            Some(ast::ModuleExportName::Str(s)) => {
                                Some(s.value.to_string_lossy().into_owned())
                            }
                            None => Some(named.local.sym.to_string()),
                        }
                    }
                    ast::ImportSpecifier::Default(_) => Some("default".into()),
                    ast::ImportSpecifier::Namespace(_) => None,
                    _ => continue,
                };
                self.module_member(&source, key.as_deref());
            }
        }
        import.visit_children_with(self);
    }

    fn visit_named_export(&mut self, export: &ast::NamedExport) {
        if !export.type_only {
            if let Some(source) = &export.src {
                let source = source.value.to_string_lossy();
                self.native_module(&source);
                for specifier in &export.specifiers {
                    let key = match specifier {
                        ast::ExportSpecifier::Named(named) if !named.is_type_only => {
                            match &named.orig {
                                ast::ModuleExportName::Ident(i) => Some(i.sym.to_string()),
                                ast::ModuleExportName::Str(s) => {
                                    Some(s.value.to_string_lossy().into_owned())
                                }
                            }
                        }
                        ast::ExportSpecifier::Namespace(_) => None,
                        _ => continue,
                    };
                    self.module_member(&source, key.as_deref());
                }
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
            let source = export.src.value.to_string_lossy();
            self.native_module(&source);
            self.module_member(&source, None);
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
            ast::Lit::Regex(_) => self.instance("RegExp"),
            _ => {}
        }
    }

    fn visit_function(&mut self, function: &ast::Function) {
        self.objects();
        self.functions();
        if function.is_async {
            self.instance("Promise");
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
        self.functions();
        if arrow.is_async {
            self.instance("Promise");
            self.add("%AsyncFunctionPrototype%");
        }
        arrow.visit_children_with(self);
    }

    fn visit_await_expr(&mut self, expr: &ast::AwaitExpr) {
        self.instance("Promise");
        if !primitive(&expr.arg) {
            self.out.receiver_keys.extend(
                crate::realm_manifest::RealmProtocol::await_receiver_keys()
                    .iter()
                    .map(|k| k.to_string()),
            );
        }
        expr.visit_children_with(self);
    }

    fn visit_for_of_stmt(&mut self, stmt: &ast::ForOfStmt) {
        self.iterables(&stmt.right);
        if stmt.is_await {
            self.instance("Promise");
            self.add("%AsyncIteratorPrototype%");
            self.add("%AsyncGeneratorPrototype%");
        }
        stmt.visit_children_with(self);
    }

    fn visit_spread_element(&mut self, spread: &ast::SpreadElement) {
        self.reflect(&spread.expr, KeylessRead::Enumerable);
        spread.visit_children_with(self);
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
        if pat
            .elems
            .iter()
            .flatten()
            .any(|p| matches!(p, ast::Pat::Rest(_)))
        {
            self.instance("Array");
        }
        for p in [
            crate::realm_manifest::RealmProtocol::Iterate,
            crate::realm_manifest::RealmProtocol::IterNext,
            crate::realm_manifest::RealmProtocol::IterResult,
        ] {
            self.out
                .receiver_keys
                .extend(p.receiver_keys().iter().map(|k| k.to_string()));
        }
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
        for prop in &pat.props {
            match prop {
                ast::ObjectPatProp::KeyValue(kv) => {
                    let key = match &kv.key {
                        ast::PropName::Ident(i) => Some(i.sym.to_string()),
                        ast::PropName::Str(s) => Some(s.value.to_string_lossy().into_owned()),
                        ast::PropName::Computed(c) => self.static_key(&c.expr),
                        _ => None,
                    };
                    match key {
                        Some(key) => {
                            self.out.receiver_keys.insert(key);
                        }
                        None => self.out.any_receiver_key = true,
                    }
                }
                ast::ObjectPatProp::Assign(p) => {
                    self.out.receiver_keys.insert(p.key.sym.to_string());
                }
                ast::ObjectPatProp::Rest(_) => {
                    self.out.keyless_read =
                        self.out.keyless_read.max(Some(KeylessRead::Enumerable));
                    self.objects();
                }
            }
        }
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
        if let Some(kind) = self.holder(&expression.callee) {
            if let Some(proto) = self
                .entries
                .last(&kind)
                .and_then(|r| r.instance_prototype())
            {
                self.out.chain_roots.insert(proto.to_string());
            }
        }
        if self.holder(&expression.callee).is_none() {
            self.objects();
        }
        if let Some(args) = &expression.args {
            self.call_events(&expression.callee, args);
        }
        expression.visit_children_with(self);
    }

    fn visit_class(&mut self, class: &ast::Class) {
        self.objects();
        self.functions();
        class.visit_children_with(self);
    }

    fn visit_bin_expr(&mut self, expr: &ast::BinExpr) {
        use crate::realm_manifest::RealmProtocol;
        use ast::BinaryOp::*;
        match expr.op {
            InstanceOf => {
                self.protocol_operand(&expr.right, RealmProtocol::HasInstance);
                expr.left.visit_with(self);
                return;
            }
            Add | Lt | LtEq | Gt | GtEq | EqEq | NotEq => {
                self.protocol_operand(&expr.left, RealmProtocol::ToPrimitive);
                self.protocol_operand(&expr.right, RealmProtocol::ToPrimitive);
                return;
            }
            _ => {}
        }
        expr.visit_children_with(self);
    }

    fn visit_ts_interface_decl(&mut self, _: &ast::TsInterfaceDecl) {}
    fn visit_ts_type_alias_decl(&mut self, _: &ast::TsTypeAliasDecl) {}

    fn visit_tpl(&mut self, tpl: &ast::Tpl) {
        for expr in &tpl.exprs {
            self.protocol_operand(expr, crate::realm_manifest::RealmProtocol::ToPrimitive);
        }
    }
}

#[path = "realm_reach_events.rs"]
mod events;
use events::primitive;

#[cfg(test)]
#[path = "realm_reach_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "realm_reach_partition_tests.rs"]
mod partition_tests;
