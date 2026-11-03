//! What one module's top-level items declare, mention and evaluate, read
//! from its AST as lowering receives it. A mention is any identifier naming
//! a module-scope binding, type positions and shadowing locals included: a
//! mention too many keeps a declaration, never drops one.

use std::collections::{BTreeSet, HashSet};

use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

/// One module's summary.
#[derive(Debug, Clone, Default)]
pub(crate) struct Summary {
    /// The item count and spans the plan was made for: the second walk
    /// prunes only the same body.
    pub fingerprint: u64,
    /// False when any binding can be read by name at run time (direct
    /// `eval`, `with`) or the module is not an ES module (`export =`,
    /// a CommonJS wrap): every item stays and every import is used whole.
    pub prunable: bool,
    pub items: Vec<Item>,
    /// Locals whose value is an ordinary `function` that reads `this`: a
    /// namespace call `ns.f()` passes the namespace as `this`.
    pub this_reading: HashSet<String>,
    /// Locals declared only by class or function declarations: valid
    /// `extends` targets once initialized.
    pub constructors: HashSet<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Item {
    pub kind: Kind,
    /// Module-scope bindings this item creates.
    pub declares: Vec<String>,
    /// Module-scope bindings this item mentions.
    pub mentions: BTreeSet<String>,
    /// `ns.member` reads of a namespace import, as (namespace, member).
    pub members: BTreeSet<(String, String)>,
    /// Namespace imports this item uses other than as `ns.member`.
    pub escapes: BTreeSet<String>,
    pub eval: Eval,
    /// (exported, local) for a local export; `default` is the local of an
    /// anonymous default export.
    pub exports: Vec<(String, String)>,
    /// (exported, imported) of `export { a as b } from`, imported `None`
    /// for `export * as b from`.
    pub reexports: Vec<(String, Option<String>)>,
    /// The specifier source of an import or re-export.
    pub source: Option<String>,
    /// (local, imported) of an import; imported `None` for a namespace.
    pub imports: Vec<(String, Option<String>)>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A declaration, a default export or a statement.
    #[default]
    Other,
    Import,
    /// `export { a, b as c }` without a source.
    ExportLocal,
    /// `export { a } from` / `export * as ns from`.
    ReExport,
    /// `export * from`.
    ExportAll,
    /// No runtime effect at all (types, `declare`, type-only edges).
    Erased,
}

/// What evaluating an item can do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum Eval {
    /// May run user code, read an unknown binding or throw: always kept.
    #[default]
    Effect,
    /// Creates its bindings and nothing else, given the conditions on
    /// imported bindings (checked against the module graph).
    Inert(Vec<Cond>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Cond {
    /// The imported binding is initialized when this item runs: its module
    /// finished evaluating first (it is not in this module's import cycle).
    Initialized(String),
    /// Also, it is a class or an ordinary function (a valid `extends`).
    Constructor(String),
}

pub(crate) fn summarize(module: &ast::Module, cjs_wrapped: bool) -> Summary {
    let mut scope: HashSet<String> = HashSet::new();
    let mut kinds: Vec<Declared> = Vec::new();
    for (index, item) in module.body.iter().enumerate() {
        for (name, decl) in declared(item) {
            scope.insert(name.clone());
            let class = declares_class(item);
            kinds.push(Declared {
                name,
                decl,
                at: index,
                class,
            });
        }
    }
    let mut runtime_names = RuntimeNames::default();
    module.visit_with(&mut runtime_names);
    let mut prunable = !cjs_wrapped && !runtime_names.found;
    let mut this_reading = HashSet::new();
    let mut items = Vec::with_capacity(module.body.len());
    for (index, item) in module.body.iter().enumerate() {
        let mut out = Item::default();
        let mut mentions = Mentions::new(&scope);
        item.visit_with(&mut mentions);
        out.mentions = mentions.names;
        out.members = mentions.members;
        out.escapes = mentions.escapes;
        out.declares = declared(item).into_iter().map(|(n, _)| n).collect();
        let order = Order {
            kinds: &kinds,
            at: index,
        };
        match item {
            ast::ModuleItem::ModuleDecl(decl) => match decl {
                ast::ModuleDecl::Import(import) => {
                    out.kind = if import.type_only {
                        Kind::Erased
                    } else {
                        Kind::Import
                    };
                    out.source = Some(import.src.value.to_string_lossy().into_owned());
                    out.imports = import_locals(import);
                    out.eval = Eval::Inert(Vec::new());
                }
                ast::ModuleDecl::ExportDecl(export) => {
                    out.eval = decl_eval(&export.decl, &order);
                    for name in &out.declares {
                        out.exports.push((name.clone(), name.clone()));
                    }
                    if matches!(
                        &export.decl,
                        ast::Decl::TsInterface(_) | ast::Decl::TsTypeAlias(_)
                    ) {
                        out.kind = Kind::Erased;
                    }
                    note_this(&export.decl, &mut this_reading);
                }
                ast::ModuleDecl::ExportNamed(named) => {
                    out.eval = Eval::Inert(Vec::new());
                    if named.type_only {
                        out.kind = Kind::Erased;
                    } else if let Some(src) = &named.src {
                        out.kind = Kind::ReExport;
                        out.source = Some(src.value.to_string_lossy().into_owned());
                        out.reexports = reexported(named);
                    } else {
                        out.kind = Kind::ExportLocal;
                        for spec in &named.specifiers {
                            if let ast::ExportSpecifier::Named(spec) = spec {
                                if spec.is_type_only {
                                    continue;
                                }
                                let local = export_name(&spec.orig);
                                let exported = spec
                                    .exported
                                    .as_ref()
                                    .map_or_else(|| local.clone(), export_name);
                                out.exports.push((exported, local));
                            }
                        }
                    }
                }
                ast::ModuleDecl::ExportAll(all) => {
                    out.kind = if all.type_only {
                        Kind::Erased
                    } else {
                        Kind::ExportAll
                    };
                    out.source = Some(all.src.value.to_string_lossy().into_owned());
                    out.eval = Eval::Inert(Vec::new());
                }
                ast::ModuleDecl::ExportDefaultDecl(default) => {
                    let local = out
                        .declares
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "default".into());
                    out.exports.push(("default".into(), local.clone()));
                    out.eval = match &default.decl {
                        ast::DefaultDecl::Fn(f) => {
                            if function_reads_this(&f.function) {
                                this_reading.insert(local);
                            }
                            fn_eval(&f.function)
                        }
                        ast::DefaultDecl::Class(c) => class_eval(&c.class, &order),
                        ast::DefaultDecl::TsInterfaceDecl(_) => {
                            out.kind = Kind::Erased;
                            Eval::Inert(Vec::new())
                        }
                    };
                    if out.declares.is_empty() {
                        out.declares.push("default".into());
                    }
                }
                ast::ModuleDecl::ExportDefaultExpr(default) => {
                    out.exports.push(("default".into(), "default".into()));
                    out.declares.push("default".into());
                    out.eval = expr_eval(&default.expr, &order);
                }
                ast::ModuleDecl::TsImportEquals(_)
                | ast::ModuleDecl::TsExportAssignment(_)
                | ast::ModuleDecl::TsNamespaceExport(_) => {
                    prunable = false;
                }
            },
            ast::ModuleItem::Stmt(stmt) => match stmt {
                ast::Stmt::Decl(decl) => {
                    out.eval = decl_eval(decl, &order);
                    if matches!(decl, ast::Decl::TsInterface(_) | ast::Decl::TsTypeAlias(_)) {
                        out.kind = Kind::Erased;
                    }
                    note_this(decl, &mut this_reading);
                }
                ast::Stmt::Empty(_) => out.eval = Eval::Inert(Vec::new()),
                ast::Stmt::Expr(e) if matches!(&*e.expr, ast::Expr::Lit(ast::Lit::Str(_))) => {
                    // A directive prologue entry: kept, it is not a binding.
                }
                _ => {}
            },
        }
        // An import or an export list names bindings without reading
        // them: they live only through the items that do and the demand.
        if matches!(
            out.kind,
            Kind::Import | Kind::ExportLocal | Kind::ReExport | Kind::ExportAll
        ) {
            out.mentions.clear();
            out.members.clear();
            out.escapes.clear();
        }
        items.push(out);
    }
    let constructors = kinds
        .iter()
        .filter(|d| {
            kinds
                .iter()
                .filter(|o| o.name == d.name)
                .all(|o| o.class || o.decl == Decl::Function)
        })
        .map(|d| d.name.clone())
        .collect();
    Summary {
        fingerprint: fingerprint(module),
        prunable,
        items,
        this_reading,
        constructors,
    }
}

/// The item count and every item's span.
pub(crate) fn fingerprint(module: &ast::Module) -> u64 {
    use std::hash::{Hash, Hasher};
    use swc_common::Spanned;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    module.body.len().hash(&mut h);
    for item in &module.body {
        let span = item.span();
        (span.lo.0, span.hi.0).hash(&mut h);
    }
    h.finish()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decl {
    /// A hoisted function: initialized before any item runs.
    Function,
    /// A class, `let`, `const` or `var`: initialized by its item.
    Lexical,
    Import,
}

/// A module-scope binding: how it is initialized, by which item.
struct Declared {
    name: String,
    decl: Decl,
    at: usize,
    class: bool,
}

fn declares_class(item: &ast::ModuleItem) -> bool {
    matches!(
        item,
        ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Class(_)))
            | ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(ast::ExportDecl {
                decl: ast::Decl::Class(_),
                ..
            }))
    ) || matches!(
        item,
        ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDefaultDecl(ast::ExportDefaultDecl {
            decl: ast::DefaultDecl::Class(_),
            ..
        }))
    )
}

fn declared(item: &ast::ModuleItem) -> Vec<(String, Decl)> {
    let mut out = Vec::new();
    match item {
        ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import)) => {
            if !import.type_only {
                for (local, _) in import_locals(import) {
                    out.push((local, Decl::Import));
                }
            }
        }
        ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => {
            decl_names(&export.decl, &mut out)
        }
        ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDefaultDecl(default)) => {
            match &default.decl {
                ast::DefaultDecl::Fn(f) => {
                    if let Some(id) = &f.ident {
                        out.push((id.sym.to_string(), Decl::Function));
                    }
                }
                ast::DefaultDecl::Class(c) => {
                    if let Some(id) = &c.ident {
                        out.push((id.sym.to_string(), Decl::Lexical));
                    }
                }
                ast::DefaultDecl::TsInterfaceDecl(_) => {}
            }
        }
        ast::ModuleItem::Stmt(ast::Stmt::Decl(decl)) => decl_names(decl, &mut out),
        _ => {}
    }
    out
}

fn decl_names(decl: &ast::Decl, out: &mut Vec<(String, Decl)>) {
    match decl {
        ast::Decl::Fn(f) => out.push((f.ident.sym.to_string(), Decl::Function)),
        ast::Decl::Class(c) => out.push((c.ident.sym.to_string(), Decl::Lexical)),
        ast::Decl::Var(var) => {
            for d in &var.decls {
                let mut names = PatNames::default();
                d.name.visit_with(&mut names);
                out.extend(names.0.into_iter().map(|n| (n, Decl::Lexical)));
            }
        }
        ast::Decl::TsEnum(e) => out.push((e.id.sym.to_string(), Decl::Lexical)),
        ast::Decl::TsModule(m) => {
            if let ast::TsModuleName::Ident(id) = &m.id {
                out.push((id.sym.to_string(), Decl::Lexical));
            }
        }
        ast::Decl::Using(using) => {
            for d in &using.decls {
                let mut names = PatNames::default();
                d.name.visit_with(&mut names);
                out.extend(names.0.into_iter().map(|n| (n, Decl::Lexical)));
            }
        }
        ast::Decl::TsInterface(_) | ast::Decl::TsTypeAlias(_) => {}
    }
}

#[derive(Default)]
struct PatNames(Vec<String>);
impl Visit for PatNames {
    fn visit_binding_ident(&mut self, id: &ast::BindingIdent) {
        self.0.push(id.id.sym.to_string());
    }
    // Default values and computed keys are expressions, not bindings.
    fn visit_expr(&mut self, _: &ast::Expr) {}
    fn visit_prop_name(&mut self, _: &ast::PropName) {}
}

fn import_locals(import: &ast::ImportDecl) -> Vec<(String, Option<String>)> {
    import
        .specifiers
        .iter()
        .filter_map(|spec| match spec {
            ast::ImportSpecifier::Named(named) if !named.is_type_only => Some((
                named.local.sym.to_string(),
                Some(
                    named
                        .imported
                        .as_ref()
                        .map_or_else(|| named.local.sym.to_string(), export_name),
                ),
            )),
            ast::ImportSpecifier::Named(_) => None,
            ast::ImportSpecifier::Default(d) => {
                Some((d.local.sym.to_string(), Some("default".into())))
            }
            ast::ImportSpecifier::Namespace(ns) => Some((ns.local.sym.to_string(), None)),
        })
        .collect()
}

fn reexported(named: &ast::NamedExport) -> Vec<(String, Option<String>)> {
    named
        .specifiers
        .iter()
        .filter_map(|spec| match spec {
            ast::ExportSpecifier::Named(spec) if !spec.is_type_only => {
                let imported = export_name(&spec.orig);
                let exported = spec
                    .exported
                    .as_ref()
                    .map_or_else(|| imported.clone(), export_name);
                Some((exported, Some(imported)))
            }
            ast::ExportSpecifier::Named(_) => None,
            ast::ExportSpecifier::Namespace(ns) => Some((export_name(&ns.name), None)),
            ast::ExportSpecifier::Default(d) => {
                Some((d.exported.sym.to_string(), Some("default".into())))
            }
        })
        .collect()
}

pub(crate) fn export_name(name: &ast::ModuleExportName) -> String {
    match name {
        ast::ModuleExportName::Ident(id) => id.sym.to_string(),
        ast::ModuleExportName::Str(s) => s.value.to_string_lossy().into_owned(),
    }
}

/// Direct `eval` and `with` read bindings by a name only known at run time.
#[derive(Default)]
struct RuntimeNames {
    found: bool,
}
impl Visit for RuntimeNames {
    fn visit_call_expr(&mut self, call: &ast::CallExpr) {
        if let ast::Callee::Expr(callee) = &call.callee {
            if matches!(&**callee, ast::Expr::Ident(id) if &*id.sym == "eval") {
                self.found = true;
            }
        }
        call.visit_children_with(self);
    }
    fn visit_with_stmt(&mut self, _: &ast::WithStmt) {
        self.found = true;
    }
}

/// Every module-scope name an item mentions; namespace imports used only as
/// `ns.member` (value or type) are recorded by member instead.
struct Mentions<'a> {
    scope: &'a HashSet<String>,
    names: BTreeSet<String>,
    members: BTreeSet<(String, String)>,
    escapes: BTreeSet<String>,
}

impl<'a> Mentions<'a> {
    fn new(scope: &'a HashSet<String>) -> Self {
        Self {
            scope,
            names: BTreeSet::new(),
            members: BTreeSet::new(),
            escapes: BTreeSet::new(),
        }
    }
    fn ident(&mut self, sym: &str) {
        if self.scope.contains(sym) {
            self.names.insert(sym.to_string());
            self.escapes.insert(sym.to_string());
        }
    }
    fn member(&mut self, ns: &str, member: &str) {
        if self.scope.contains(ns) {
            self.names.insert(ns.to_string());
            self.members.insert((ns.to_string(), member.to_string()));
        }
    }
}

impl Visit for Mentions<'_> {
    fn visit_ident(&mut self, id: &ast::Ident) {
        self.ident(&id.sym);
    }
    fn visit_member_expr(&mut self, m: &ast::MemberExpr) {
        if let (ast::Expr::Ident(obj), ast::MemberProp::Ident(prop)) = (&*m.obj, &m.prop) {
            self.member(&obj.sym, &prop.sym);
            return;
        }
        m.visit_children_with(self);
    }
    fn visit_ts_qualified_name(&mut self, q: &ast::TsQualifiedName) {
        if let ast::TsEntityName::Ident(left) = &q.left {
            self.member(&left.sym, &q.right.sym);
            return;
        }
        q.visit_children_with(self);
    }
    fn visit_jsx_member_expr(&mut self, m: &ast::JSXMemberExpr) {
        if let ast::JSXObject::Ident(obj) = &m.obj {
            self.member(&obj.sym, &m.prop.sym);
            return;
        }
        m.visit_children_with(self);
    }
}

/// What the plan needs to order an item's reads: which names are hoisted
/// functions, which are declared by an earlier item, which are imports.
struct Order<'a> {
    kinds: &'a [Declared],
    at: usize,
}

impl Order<'_> {
    fn decl(&self, name: &str) -> Option<Decl> {
        self.kinds.iter().find(|d| d.name == name).map(|d| d.decl)
    }
    /// Every declaration of `name` is an item before this one.
    fn earlier(&self, name: &str) -> bool {
        self.kinds
            .iter()
            .filter(|d| d.name == name)
            .all(|d| d.at < self.at)
    }
    /// `name` is a class declaration (and nothing else).
    fn class(&self, name: &str) -> bool {
        self.kinds
            .iter()
            .filter(|d| d.name == name)
            .all(|d| d.class)
    }
}

fn decl_eval(decl: &ast::Decl, order: &Order) -> Eval {
    match decl {
        ast::Decl::Fn(f) => fn_eval(&f.function),
        ast::Decl::Class(c) => class_eval(&c.class, order),
        ast::Decl::Var(var) => {
            if var.declare {
                return Eval::Inert(Vec::new());
            }
            let mut conds = Vec::new();
            for d in &var.decls {
                // Destructuring can run iterators and getters.
                if !matches!(d.name, ast::Pat::Ident(_)) {
                    return Eval::Effect;
                }
                if let Some(init) = &d.init {
                    match expr_eval(init, order) {
                        Eval::Inert(c) => conds.extend(c),
                        Eval::Effect => return Eval::Effect,
                    }
                }
            }
            Eval::Inert(conds)
        }
        ast::Decl::TsInterface(_) | ast::Decl::TsTypeAlias(_) => Eval::Inert(Vec::new()),
        ast::Decl::TsEnum(e) => {
            if e.declare {
                return Eval::Inert(Vec::new());
            }
            let literal = e.members.iter().all(|m| {
                m.init.as_deref().is_none_or(|i| {
                    matches!(i, ast::Expr::Lit(ast::Lit::Num(_) | ast::Lit::Str(_)))
                })
            });
            if literal {
                Eval::Inert(Vec::new())
            } else {
                Eval::Effect
            }
        }
        ast::Decl::TsModule(m) if m.declare => Eval::Inert(Vec::new()),
        ast::Decl::TsModule(_) | ast::Decl::Using(_) => Eval::Effect,
    }
}

fn fn_eval(f: &ast::Function) -> Eval {
    // Parameters and the body run on a call; decorators would run now.
    if f.decorators.is_empty() && f.params.iter().all(|p| p.decorators.is_empty()) {
        Eval::Inert(Vec::new())
    } else {
        Eval::Effect
    }
}

fn class_eval(class: &ast::Class, order: &Order) -> Eval {
    if !class.decorators.is_empty() {
        return Eval::Effect;
    }
    let mut conds = Vec::new();
    if let Some(sup) = &class.super_class {
        match &**sup {
            ast::Expr::Lit(ast::Lit::Null(_)) => {}
            ast::Expr::Ident(id) => match order.decl(&id.sym) {
                Some(Decl::Function) => {}
                Some(Decl::Lexical) if order.earlier(&id.sym) && order.class(&id.sym) => {}
                Some(Decl::Import) => conds.push(Cond::Constructor(id.sym.to_string())),
                _ => return Eval::Effect,
            },
            _ => return Eval::Effect,
        }
    }
    for member in &class.body {
        let inert = match member {
            ast::ClassMember::Constructor(c) => c.params.iter().all(|p| match p {
                ast::ParamOrTsParamProp::Param(p) => p.decorators.is_empty(),
                ast::ParamOrTsParamProp::TsParamProp(p) => p.decorators.is_empty(),
            }),
            ast::ClassMember::Method(m) => {
                key_is_literal(&m.key)
                    && m.function.decorators.is_empty()
                    && m.function.params.iter().all(|p| p.decorators.is_empty())
            }
            ast::ClassMember::PrivateMethod(m) => {
                m.function.decorators.is_empty()
                    && m.function.params.iter().all(|p| p.decorators.is_empty())
            }
            ast::ClassMember::ClassProp(p) => {
                key_is_literal(&p.key)
                    && p.decorators.is_empty()
                    && (!p.is_static
                        || p.value
                            .as_deref()
                            .is_none_or(|v| match expr_eval(v, order) {
                                Eval::Inert(c) => {
                                    conds.extend(c);
                                    true
                                }
                                Eval::Effect => false,
                            }))
            }
            ast::ClassMember::PrivateProp(p) => {
                p.decorators.is_empty()
                    && (!p.is_static
                        || p.value
                            .as_deref()
                            .is_none_or(|v| match expr_eval(v, order) {
                                Eval::Inert(c) => {
                                    conds.extend(c);
                                    true
                                }
                                Eval::Effect => false,
                            }))
            }
            ast::ClassMember::TsIndexSignature(_) | ast::ClassMember::Empty(_) => true,
            ast::ClassMember::StaticBlock(_) => false,
            ast::ClassMember::AutoAccessor(a) => {
                let key_ok = match &a.key {
                    ast::Key::Private(_) => true,
                    ast::Key::Public(k) => key_is_literal(k),
                };
                key_ok && a.decorators.is_empty() && (!a.is_static || a.value.is_none())
            }
        };
        if !inert {
            return Eval::Effect;
        }
    }
    Eval::Inert(conds)
}

fn key_is_literal(key: &ast::PropName) -> bool {
    match key {
        ast::PropName::Ident(_) | ast::PropName::Str(_) | ast::PropName::Num(_) => true,
        ast::PropName::Computed(c) => matches!(
            &*c.expr,
            ast::Expr::Lit(ast::Lit::Str(_) | ast::Lit::Num(_))
        ),
        ast::PropName::BigInt(_) => true,
    }
}

/// Evaluating `expr` creates a value and nothing else.
fn expr_eval(expr: &ast::Expr, order: &Order) -> Eval {
    let mut conds = Vec::new();
    if inert_expr(expr, order, &mut conds) {
        Eval::Inert(conds)
    } else {
        Eval::Effect
    }
}

fn inert_expr(expr: &ast::Expr, order: &Order, conds: &mut Vec<Cond>) -> bool {
    match expr {
        ast::Expr::Lit(lit) => !matches!(lit, ast::Lit::JSXText(_)),
        ast::Expr::Fn(f) => matches!(fn_eval(&f.function), Eval::Inert(_)),
        ast::Expr::Arrow(_) => true,
        ast::Expr::Class(c) => match class_eval(&c.class, order) {
            Eval::Inert(c) => {
                conds.extend(c);
                true
            }
            Eval::Effect => false,
        },
        ast::Expr::Tpl(t) => t.exprs.is_empty(),
        ast::Expr::Paren(p) => inert_expr(&p.expr, order, conds),
        ast::Expr::TsAs(e) => inert_expr(&e.expr, order, conds),
        ast::Expr::TsSatisfies(e) => inert_expr(&e.expr, order, conds),
        ast::Expr::TsTypeAssertion(e) => inert_expr(&e.expr, order, conds),
        ast::Expr::TsConstAssertion(e) => inert_expr(&e.expr, order, conds),
        ast::Expr::TsNonNull(e) => inert_expr(&e.expr, order, conds),
        ast::Expr::Array(a) => a
            .elems
            .iter()
            .flatten()
            .all(|e| e.spread.is_none() && inert_expr(&e.expr, order, conds)),
        ast::Expr::Object(o) => o.props.iter().all(|p| match p {
            ast::PropOrSpread::Spread(_) => false,
            ast::PropOrSpread::Prop(p) => match &**p {
                ast::Prop::KeyValue(kv) => {
                    key_is_literal(&kv.key) && inert_expr(&kv.value, order, conds)
                }
                ast::Prop::Shorthand(id) => inert_read(&id.sym, order, conds),
                ast::Prop::Method(m) => key_is_literal(&m.key),
                ast::Prop::Getter(g) => key_is_literal(&g.key),
                ast::Prop::Setter(s) => key_is_literal(&s.key),
                ast::Prop::Assign(_) => false,
            },
        }),
        ast::Expr::Ident(id) => inert_read(&id.sym, order, conds),
        ast::Expr::Unary(u)
            if matches!(
                u.op,
                ast::UnaryOp::TypeOf | ast::UnaryOp::Void | ast::UnaryOp::Bang
            ) =>
        {
            inert_expr(&u.arg, order, conds)
        }
        ast::Expr::Unary(u) if matches!(u.op, ast::UnaryOp::Minus | ast::UnaryOp::Plus) => {
            matches!(&*u.arg, ast::Expr::Lit(ast::Lit::Num(_)))
        }
        ast::Expr::Cond(c) => {
            inert_expr(&c.test, order, conds)
                && inert_expr(&c.cons, order, conds)
                && inert_expr(&c.alt, order, conds)
        }
        ast::Expr::Seq(s) => s.exprs.iter().all(|e| inert_expr(e, order, conds)),
        // Even a member read can call a getter; calls and `new` run code.
        _ => false,
    }
}

/// Reading `name` here cannot throw: a hoisted function, a binding an
/// earlier item initialized, an import whose module ran first, or the
/// global `undefined`.
fn inert_read(name: &str, order: &Order, conds: &mut Vec<Cond>) -> bool {
    match order.decl(name) {
        Some(Decl::Function) => true,
        Some(Decl::Lexical) => order.earlier(name),
        Some(Decl::Import) => {
            conds.push(Cond::Initialized(name.to_string()));
            true
        }
        None => name == "undefined",
    }
}

fn note_this(decl: &ast::Decl, out: &mut HashSet<String>) {
    match decl {
        ast::Decl::Fn(f) if function_reads_this(&f.function) => {
            out.insert(f.ident.sym.to_string());
        }
        ast::Decl::Var(var) => {
            for d in &var.decls {
                if let (ast::Pat::Ident(id), Some(init)) = (&d.name, &d.init) {
                    if let ast::Expr::Fn(f) = &**init {
                        if function_reads_this(&f.function) {
                            out.insert(id.id.sym.to_string());
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn function_reads_this(f: &ast::Function) -> bool {
    #[derive(Default)]
    struct This(bool);
    impl Visit for This {
        fn visit_this_expr(&mut self, _: &ast::ThisExpr) {
            self.0 = true;
        }
    }
    let mut v = This::default();
    f.visit_with(&mut v);
    v.0
}
