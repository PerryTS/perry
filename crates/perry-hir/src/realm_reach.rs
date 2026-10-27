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

    pub fn close(&mut self, entries: &[RealmEntry]) {
        if self.is_full() {
            self.names
                .extend(entries.iter().map(|e| e.name.to_string()));
            return;
        }
        let by_name: BTreeMap<_, _> = entries.iter().map(|e| (e.name, e)).collect();
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
    let mut resolved = module.clone();
    resolved.visit_mut_with(&mut Clear);
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
        resolved.visit_with(&mut Scan {
            entries,
            out,
            unresolved,
            constants: &constants.0,
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
        Expr::IndexGet { object, index }
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

struct Scan<'a> {
    entries: &'a [RealmEntry],
    out: &'a mut RealmReach,
    unresolved: Mark,
    constants: &'a BTreeMap<ast::Id, String>,
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
        matches!(crate::patched_builtins::peel(expr), ast::Expr::Ident(i) if self.free(i) && is_global(&i.sym))
    }

    fn native_module(&mut self, name: &str) {
        let name = name.strip_prefix("node:").unwrap_or(name);
        for entry in self.entries {
            if entry.module == Some(name) {
                self.out.names.insert(entry.name.into());
            }
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

impl Visit for Scan<'_> {
    fn visit_ts_type(&mut self, _: &ast::TsType) {}

    fn visit_expr(&mut self, expr: &ast::Expr) {
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
        if self.global(&member.obj) {
            let key = match &member.prop {
                ast::MemberProp::Ident(i) => Some(i.sym.to_string()),
                ast::MemberProp::Computed(c) => self.static_key(&c.expr),
                _ => None,
            };
            if let Some(key) = key {
                self.add(&key);
            } else {
                self.out.top.insert(RealmTop::GlobalObjectValue);
            }
            // A static member does not use the global object as a value.
            if let ast::MemberProp::Computed(c) = &member.prop {
                c.expr.visit_with(self);
            }
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

    fn visit_prop(&mut self, prop: &ast::Prop) {
        if let ast::Prop::Shorthand(i) = prop {
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
            }
        }
        export.visit_children_with(self);
    }

    fn visit_export_all(&mut self, export: &ast::ExportAll) {
        if !export.type_only {
            self.native_module(&export.src.value.to_string_lossy());
        }
    }

    fn visit_call_expr(&mut self, call: &ast::CallExpr) {
        let module_call = match &call.callee {
            ast::Callee::Import(_) => true,
            ast::Callee::Expr(e) => {
                matches!(crate::patched_builtins::peel(e), ast::Expr::Ident(i) if self.free(i) && i.sym == "require")
            }
            _ => false,
        };
        if module_call {
            if let Some(name) = call
                .args
                .first()
                .filter(|a| a.spread.is_none())
                .and_then(|a| constant_string(&a.expr, self.constants))
            {
                self.native_module(&name);
            } else {
                self.out.top.insert(RealmTop::DynamicModule);
            }
        }
        // Generic calls can receive every primitive kind. Literal-only
        // programs below name just the kinds present in their expressions.
        if !module_call {
            self.primitive_prototypes();
        }
        call.visit_children_with(self);
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

    fn visit_tpl(&mut self, tpl: &ast::Tpl) {
        self.add("String");
        tpl.visit_children_with(self);
    }
}

#[cfg(test)]
#[path = "realm_reach_tests.rs"]
mod tests;
