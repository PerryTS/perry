//! #6812: fold straight-line "builder" sequences into the object literal
//! they spell out, before lowering.
//!
//! ```ts
//! const o: any = {};
//! o.a = i; o.b = r + i; o.c = f(x);
//! ```
//! lowers today as an empty-object allocation plus N dynamic transition
//! writes (~500 ns each: PIC-ineligible `class_id == 0` receiver, keys-array
//! transitions, barriers). Folded into `const o = { a: i, b: r + i, c: f(x) }`
//! it flows through the anon-shape literal machinery (shape-cached keys
//! array, typed slots, direct stores) — the path that already beats node.
//!
//! Soundness argument (why the rewrite is unobservable):
//! - The appended value expressions run in the same order at the same
//!   sequence points; only the allocation moves AFTER them, and a bare
//!   object allocation has no user-visible effects.
//! - Values must not reference the bound name (checked conservatively by
//!   symbol name anywhere in the value expression, ignoring shadowing), so
//!   no expression can observe the half-built object.
//! - If a value throws, the original leaves a partially-built object bound
//!   to a local no live code can reach (the following statements never run,
//!   and the values captured no reference to it) — indistinguishable.
//! - Keys are literal identifiers / string literals only; `__proto__` is
//!   excluded (assignment triggers the prototype setter; a literal key
//!   would define a plain property). Duplicate keys stop the fold (the
//!   original overwrote in place; combined with accessors that could
//!   differ). Literals already containing accessor/spread/computed/method
//!   props are left untouched entirely — an appended key could otherwise
//!   turn a setter invocation into a redefinition.
//! - Only `Pat::Ident` bindings qualify; the declarator may carry any type
//!   annotation. Exported declarations are skipped (scope kept tight).
//!
//! A miss here is only a missed optimization: unmatched shapes lower
//! exactly as before.

use swc_ecma_ast as ast;

/// Fold cap per literal — beyond this the object is dictionary-like and the
/// literal machinery's inline-slot benefits taper off anyway.
const MAX_FOLDED_PROPS: usize = 64;

/// Returns a folded clone when at least one builder sequence was folded;
/// `None` means "nothing to do — lower the original".
pub(crate) fn fold_builder_sequences(module: &ast::Module) -> Option<ast::Module> {
    if !module_has_candidate(module) {
        return None;
    }
    let mut folded = module.clone();
    let mut changed = false;
    process_module_items(&mut folded.body, &mut changed);
    changed.then_some(folded)
}

/// Cheap read-only pre-scan: is there any `const/let/var x = {…}` statement
/// list anywhere that is immediately followed by a static member assignment
/// to the same name? (False positives are fine — they only cost the clone.)
fn module_has_candidate(module: &ast::Module) -> bool {
    struct Scan {
        found: bool,
    }
    impl Scan {
        fn stmts(&mut self, stmts: &[ast::Stmt]) {
            if self.found {
                return;
            }
            for pair in stmts.windows(2) {
                if let (Some(name), _) = decl_object_binding(&pair[0]) {
                    if assign_to_name_key(&pair[1], name.as_str()).is_some() {
                        self.found = true;
                        return;
                    }
                }
            }
            for s in stmts {
                self.stmt(s);
            }
        }
        fn stmt(&mut self, s: &ast::Stmt) {
            if self.found {
                return;
            }
            match s {
                ast::Stmt::Block(b) => self.stmts(&b.stmts),
                ast::Stmt::If(i) => {
                    self.stmt(&i.cons);
                    if let Some(alt) = &i.alt {
                        self.stmt(alt);
                    }
                }
                ast::Stmt::While(w) => self.stmt(&w.body),
                ast::Stmt::DoWhile(d) => self.stmt(&d.body),
                ast::Stmt::For(f) => self.stmt(&f.body),
                ast::Stmt::ForIn(f) => self.stmt(&f.body),
                ast::Stmt::ForOf(f) => self.stmt(&f.body),
                ast::Stmt::Labeled(l) => self.stmt(&l.body),
                ast::Stmt::Try(t) => {
                    self.stmts(&t.block.stmts);
                    if let Some(h) = &t.handler {
                        self.stmts(&h.body.stmts);
                    }
                    if let Some(f) = &t.finalizer {
                        self.stmts(&f.stmts);
                    }
                }
                ast::Stmt::Switch(sw) => {
                    for case in &sw.cases {
                        self.stmts(&case.cons);
                    }
                }
                ast::Stmt::Decl(ast::Decl::Fn(f)) => {
                    if let Some(body) = &f.function.body {
                        self.stmts(&body.stmts);
                    }
                }
                _ => {}
            }
        }
    }
    let mut scan = Scan { found: false };
    // Top level: treat consecutive ModuleItem::Stmt entries as a window.
    for pair in module.body.windows(2) {
        if let (ast::ModuleItem::Stmt(a), ast::ModuleItem::Stmt(b)) = (&pair[0], &pair[1]) {
            if let (Some(name), _) = decl_object_binding(a) {
                if assign_to_name_key(b, name.as_str()).is_some() {
                    return true;
                }
            }
        }
    }
    for item in &module.body {
        match item {
            ast::ModuleItem::Stmt(s) => scan.stmt(s),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(ed)) => {
                if let ast::Decl::Fn(f) = &ed.decl {
                    if let Some(body) = &f.function.body {
                        scan.stmts(&body.stmts);
                    }
                }
            }
            _ => {}
        }
        if scan.found {
            return true;
        }
    }
    // Function bodies nested in expressions are found during the mutating
    // walk; missing them here only skips the fold for modules whose ONLY
    // candidates hide inside expression-nested functions. Cover the common
    // case cheaply: any module containing an arrow/function expression gets
    // the full walk.
    module_contains_function_expr(module) || scan.found
}

fn module_contains_function_expr(_m: &ast::Module) -> bool {
    // Conservative: assume yes. The clone cost is paid once per module and
    // the mutating walk is linear; modules with no candidates simply come
    // back unchanged (changed == false → original is used).
    true
}

fn process_module_items(items: &mut [ast::ModuleItem], changed: &mut bool) {
    // Fold across consecutive top-level Stmt items.
    let mut i = 0;
    while i < items.len() {
        if let ast::ModuleItem::Stmt(_) = &items[i] {
            // Collect the run of plain statements [i, j).
            let mut j = i;
            while j < items.len() && matches!(items[j], ast::ModuleItem::Stmt(_)) {
                j += 1;
            }
            // Temporarily extract the run as &mut [Stmt]-alike processing.
            fold_module_stmt_run(&mut items[i..j], changed);
            for item in items[i..j].iter_mut() {
                if let ast::ModuleItem::Stmt(s) = item {
                    walk_stmt(s, changed);
                }
            }
            i = j;
        } else {
            if let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(ed)) = &mut items[i] {
                walk_decl(&mut ed.decl, changed);
            }
            i += 1;
        }
    }
}

/// Fold within a run of top-level ModuleItem::Stmt entries. Consumed
/// assignment statements are replaced with `;` (EmptyStmt).
fn fold_module_stmt_run(items: &mut [ast::ModuleItem], changed: &mut bool) {
    let mut idx = 0;
    while idx < items.len() {
        let Some((name_start, existing)) = ({
            match &items[idx] {
                ast::ModuleItem::Stmt(s) => match decl_object_binding(s) {
                    (Some(name), Some(props)) => Some((name.clone(), props)),
                    _ => None,
                },
                _ => None,
            }
        }) else {
            idx += 1;
            continue;
        };
        if !literal_is_foldable(existing) {
            idx += 1;
            continue;
        }
        let mut keys = existing_keys(existing);
        let mut appended: Vec<(ast::PropName, Box<ast::Expr>)> = Vec::new();
        let mut consumed = 0usize;
        for follower in items[idx + 1..].iter() {
            let ast::ModuleItem::Stmt(fs) = follower else {
                break;
            };
            let Some((key, value)) = assign_to_name_key(fs, &name_start) else {
                break;
            };
            if !fold_key_ok(&key, &keys) || expr_references_ident(value, &name_start) {
                break;
            }
            if existing.len() + appended.len() >= MAX_FOLDED_PROPS {
                break;
            }
            keys.push(prop_name_atom(&key));
            appended.push((key, Box::new((**value).clone())));
            consumed += 1;
        }
        if consumed == 0 {
            idx += 1;
            continue;
        }
        // Apply: extend the literal, blank out the consumed statements.
        if let ast::ModuleItem::Stmt(s) = &mut items[idx] {
            append_props(s, appended);
        }
        for follower in items[idx + 1..idx + 1 + consumed].iter_mut() {
            *follower = ast::ModuleItem::Stmt(ast::Stmt::Empty(ast::EmptyStmt {
                span: swc_common::DUMMY_SP,
            }));
        }
        *changed = true;
        idx += 1 + consumed;
    }
}

fn fold_stmts(stmts: &mut Vec<ast::Stmt>, changed: &mut bool) {
    let mut idx = 0;
    while idx < stmts.len() {
        let foldable = match decl_object_binding(&stmts[idx]) {
            (Some(name), Some(props)) if literal_is_foldable(props) => {
                Some((name.clone(), existing_keys(props), props.len()))
            }
            _ => None,
        };
        let Some((name, mut keys, existing_len)) = foldable else {
            idx += 1;
            continue;
        };
        let mut appended: Vec<(ast::PropName, Box<ast::Expr>)> = Vec::new();
        let mut consumed = 0usize;
        for follower in stmts[idx + 1..].iter() {
            let Some((key, value)) = assign_to_name_key(follower, &name) else {
                break;
            };
            if !fold_key_ok(&key, &keys) || expr_references_ident(value, &name) {
                break;
            }
            if existing_len + appended.len() >= MAX_FOLDED_PROPS {
                break;
            }
            keys.push(prop_name_atom(&key));
            appended.push((key, Box::new((**value).clone())));
            consumed += 1;
        }
        if consumed > 0 {
            append_props(&mut stmts[idx], appended);
            stmts.drain(idx + 1..idx + 1 + consumed);
            *changed = true;
        }
        idx += 1;
    }
    for s in stmts.iter_mut() {
        walk_stmt(s, changed);
    }
}

/// `const/let/var <ident> = { … }` → (binding name, literal props).
fn decl_object_binding(s: &ast::Stmt) -> (Option<String>, Option<&Vec<ast::PropOrSpread>>) {
    let ast::Stmt::Decl(ast::Decl::Var(var)) = s else {
        return (None, None);
    };
    if var.decls.len() != 1 {
        return (None, None);
    }
    let d = &var.decls[0];
    let ast::Pat::Ident(bi) = &d.name else {
        return (None, None);
    };
    let Some(init) = &d.init else {
        return (None, None);
    };
    let ast::Expr::Object(obj) = &**init else {
        return (None, None);
    };
    (Some(bi.id.sym.to_string()), Some(&obj.props))
}

/// `name.key = value;` or `name["key"] = value;` with a plain `=`.
fn assign_to_name_key<'a>(
    s: &'a ast::Stmt,
    name: &str,
) -> Option<(ast::PropName, &'a Box<ast::Expr>)> {
    let ast::Stmt::Expr(es) = s else { return None };
    let ast::Expr::Assign(a) = &*es.expr else {
        return None;
    };
    if a.op != ast::AssignOp::Assign {
        return None;
    }
    let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(m)) = &a.left else {
        return None;
    };
    let ast::Expr::Ident(obj) = &*m.obj else {
        return None;
    };
    if obj.sym.as_ref() != name {
        return None;
    }
    let key = match &m.prop {
        ast::MemberProp::Ident(id) => ast::PropName::Ident(id.clone()),
        ast::MemberProp::Computed(c) => match &*c.expr {
            ast::Expr::Lit(ast::Lit::Str(sl)) => ast::PropName::Str(sl.clone()),
            _ => return None,
        },
        ast::MemberProp::PrivateName(_) => return None,
    };
    Some((key, &a.right))
}

/// The literal may only contain plain key/value + shorthand props; anything
/// else (accessors, spreads, computed keys, methods) disables folding.
fn literal_is_foldable(props: &[ast::PropOrSpread]) -> bool {
    props.iter().all(|p| {
        matches!(
            p,
            ast::PropOrSpread::Prop(prop)
                if matches!(
                    &**prop,
                    ast::Prop::KeyValue(kv)
                        if matches!(kv.key, ast::PropName::Ident(_) | ast::PropName::Str(_))
                ) || matches!(&**prop, ast::Prop::Shorthand(_))
        )
    })
}

fn existing_keys(props: &[ast::PropOrSpread]) -> Vec<String> {
    props
        .iter()
        .filter_map(|p| match p {
            ast::PropOrSpread::Prop(prop) => match &**prop {
                ast::Prop::KeyValue(kv) => match &kv.key {
                    ast::PropName::Ident(i) => Some(i.sym.to_string()),
                    ast::PropName::Str(s) => s.value.as_str().map(|v| v.to_string()),
                    _ => None,
                },
                ast::Prop::Shorthand(i) => Some(i.sym.to_string()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn prop_name_atom(key: &ast::PropName) -> String {
    match key {
        ast::PropName::Ident(i) => i.sym.to_string(),
        ast::PropName::Str(s) => s.value.as_str().map(|v| v.to_string()).unwrap_or_default(),
        _ => String::new(),
    }
}

fn fold_key_ok(key: &ast::PropName, existing: &[String]) -> bool {
    let atom = prop_name_atom(key);
    if atom.is_empty() || atom == "__proto__" {
        return false;
    }
    !existing.iter().any(|k| *k == atom)
}

fn append_props(s: &mut ast::Stmt, appended: Vec<(ast::PropName, Box<ast::Expr>)>) {
    let ast::Stmt::Decl(ast::Decl::Var(var)) = s else {
        return;
    };
    let Some(init) = &mut var.decls[0].init else {
        return;
    };
    let ast::Expr::Object(obj) = &mut **init else {
        return;
    };
    for (key, value) in appended {
        obj.props
            .push(ast::PropOrSpread::Prop(Box::new(ast::Prop::KeyValue(
                ast::KeyValueProp { key, value },
            ))));
    }
}

/// Conservative: does `e` mention an identifier with this symbol name
/// anywhere (ignoring shadowing — false positives only block the fold)?
fn expr_references_ident(e: &ast::Expr, name: &str) -> bool {
    use ast::Expr as E;
    match e {
        E::Ident(i) => i.sym.as_ref() == name,
        E::Lit(_) | E::This(_) => false,
        E::Array(a) => a.elems.iter().flatten().any(|el| expr_references_ident(&el.expr, name)),
        E::Object(o) => o.props.iter().any(|p| match p {
            ast::PropOrSpread::Spread(sp) => expr_references_ident(&sp.expr, name),
            ast::PropOrSpread::Prop(prop) => match &**prop {
                ast::Prop::KeyValue(kv) => {
                    expr_references_ident(&kv.value, name)
                        || matches!(&kv.key, ast::PropName::Computed(c) if expr_references_ident(&c.expr, name))
                }
                ast::Prop::Shorthand(i) => i.sym.as_ref() == name,
                // Accessors/methods may close over the name — be safe.
                _ => true,
            },
        }),
        E::Unary(u) => expr_references_ident(&u.arg, name),
        E::Update(u) => expr_references_ident(&u.arg, name),
        E::Bin(b) => expr_references_ident(&b.left, name) || expr_references_ident(&b.right, name),
        E::Assign(a) => {
            // Any assignment inside a value expression: too clever — block.
            let _ = a;
            true
        }
        E::Member(m) => {
            expr_references_ident(&m.obj, name)
                || matches!(&m.prop, ast::MemberProp::Computed(c) if expr_references_ident(&c.expr, name))
        }
        E::SuperProp(sp) => {
            matches!(&sp.prop, ast::SuperProp::Computed(c) if expr_references_ident(&c.expr, name))
        }
        E::Cond(c) => {
            expr_references_ident(&c.test, name)
                || expr_references_ident(&c.cons, name)
                || expr_references_ident(&c.alt, name)
        }
        E::Call(c) => {
            (match &c.callee {
                ast::Callee::Expr(e) => expr_references_ident(e, name),
                _ => false,
            }) || c.args.iter().any(|a| expr_references_ident(&a.expr, name))
        }
        E::New(n) => {
            expr_references_ident(&n.callee, name)
                || n.args
                    .iter()
                    .flatten()
                    .any(|a| expr_references_ident(&a.expr, name))
        }
        E::Seq(s) => s.exprs.iter().any(|e| expr_references_ident(e, name)),
        E::Tpl(t) => t.exprs.iter().any(|e| expr_references_ident(e, name)),
        E::TaggedTpl(t) => {
            expr_references_ident(&t.tag, name)
                || t.tpl.exprs.iter().any(|e| expr_references_ident(e, name))
        }
        E::Paren(p) => expr_references_ident(&p.expr, name),
        E::Await(a) => expr_references_ident(&a.arg, name),
        E::Yield(y) => y.arg.as_deref().is_some_and(|a| expr_references_ident(a, name)),
        E::TsAs(t) => expr_references_ident(&t.expr, name),
        E::TsNonNull(t) => expr_references_ident(&t.expr, name),
        E::TsTypeAssertion(t) => expr_references_ident(&t.expr, name),
        E::TsSatisfies(t) => expr_references_ident(&t.expr, name),
        E::OptChain(oc) => match &*oc.base {
            ast::OptChainBase::Member(m) => {
                expr_references_ident(&m.obj, name)
                    || matches!(&m.prop, ast::MemberProp::Computed(c) if expr_references_ident(&c.expr, name))
            }
            ast::OptChainBase::Call(c) => {
                expr_references_ident(&c.callee, name)
                    || c.args.iter().any(|a| expr_references_ident(&a.expr, name))
            }
        },
        // Function-bearing expressions may close over the name — be safe.
        E::Arrow(_) | E::Fn(_) | E::Class(_) => true,
        // Unknown/rare forms: be safe.
        _ => true,
    }
}

fn walk_decl(d: &mut ast::Decl, changed: &mut bool) {
    if let ast::Decl::Fn(f) = d {
        if let Some(body) = &mut f.function.body {
            fold_stmts(&mut body.stmts, changed);
        }
    }
    if let ast::Decl::Class(c) = d {
        walk_class(&mut c.class, changed);
    }
}

fn walk_class(class: &mut ast::Class, changed: &mut bool) {
    for member in &mut class.body {
        match member {
            ast::ClassMember::Method(m) => {
                if let Some(body) = &mut m.function.body {
                    fold_stmts(&mut body.stmts, changed);
                }
            }
            ast::ClassMember::PrivateMethod(m) => {
                if let Some(body) = &mut m.function.body {
                    fold_stmts(&mut body.stmts, changed);
                }
            }
            ast::ClassMember::Constructor(c) => {
                if let Some(body) = &mut c.body {
                    fold_stmts(&mut body.stmts, changed);
                }
            }
            ast::ClassMember::StaticBlock(b) => fold_stmts(&mut b.body.stmts, changed),
            _ => {}
        }
    }
}

fn walk_stmt(s: &mut ast::Stmt, changed: &mut bool) {
    match s {
        ast::Stmt::Block(b) => fold_stmts(&mut b.stmts, changed),
        ast::Stmt::If(i) => {
            walk_stmt(&mut i.cons, changed);
            if let Some(alt) = &mut i.alt {
                walk_stmt(alt, changed);
            }
            walk_expr(&mut i.test, changed);
        }
        ast::Stmt::While(w) => {
            walk_expr(&mut w.test, changed);
            walk_stmt(&mut w.body, changed);
        }
        ast::Stmt::DoWhile(d) => {
            walk_stmt(&mut d.body, changed);
            walk_expr(&mut d.test, changed);
        }
        ast::Stmt::For(f) => {
            if let Some(ast::VarDeclOrExpr::Expr(e)) = &mut f.init {
                walk_expr(e, changed);
            }
            if let Some(t) = &mut f.test {
                walk_expr(t, changed);
            }
            if let Some(u) = &mut f.update {
                walk_expr(u, changed);
            }
            walk_stmt(&mut f.body, changed);
        }
        ast::Stmt::ForIn(f) => walk_stmt(&mut f.body, changed),
        ast::Stmt::ForOf(f) => walk_stmt(&mut f.body, changed),
        ast::Stmt::Labeled(l) => walk_stmt(&mut l.body, changed),
        ast::Stmt::Try(t) => {
            fold_stmts(&mut t.block.stmts, changed);
            if let Some(h) = &mut t.handler {
                fold_stmts(&mut h.body.stmts, changed);
            }
            if let Some(f) = &mut t.finalizer {
                fold_stmts(&mut f.stmts, changed);
            }
        }
        ast::Stmt::Switch(sw) => {
            walk_expr(&mut sw.discriminant, changed);
            for case in &mut sw.cases {
                fold_stmts(&mut case.cons, changed);
            }
        }
        ast::Stmt::Decl(d) => walk_decl(d, changed),
        ast::Stmt::Expr(es) => walk_expr(&mut es.expr, changed),
        ast::Stmt::Return(r) => {
            if let Some(e) = &mut r.arg {
                walk_expr(e, changed);
            }
        }
        ast::Stmt::Throw(t) => walk_expr(&mut t.arg, changed),
        _ => {}
    }
}

/// Recurse into expressions only far enough to find nested function bodies.
fn walk_expr(e: &mut ast::Expr, changed: &mut bool) {
    use ast::Expr as E;
    match e {
        E::Fn(f) => {
            if let Some(body) = &mut f.function.body {
                fold_stmts(&mut body.stmts, changed);
            }
        }
        E::Arrow(a) => match &mut *a.body {
            ast::BlockStmtOrExpr::BlockStmt(b) => fold_stmts(&mut b.stmts, changed),
            ast::BlockStmtOrExpr::Expr(e) => walk_expr(e, changed),
        },
        E::Class(c) => walk_class(&mut c.class, changed),
        E::Array(a) => {
            for el in a.elems.iter_mut().flatten() {
                walk_expr(&mut el.expr, changed);
            }
        }
        E::Object(o) => {
            for p in &mut o.props {
                match p {
                    ast::PropOrSpread::Spread(sp) => walk_expr(&mut sp.expr, changed),
                    ast::PropOrSpread::Prop(prop) => match &mut **prop {
                        ast::Prop::KeyValue(kv) => walk_expr(&mut kv.value, changed),
                        ast::Prop::Method(m) => {
                            if let Some(body) = &mut m.function.body {
                                fold_stmts(&mut body.stmts, changed);
                            }
                        }
                        ast::Prop::Getter(g) => {
                            if let Some(body) = &mut g.body {
                                fold_stmts(&mut body.stmts, changed);
                            }
                        }
                        ast::Prop::Setter(sst) => {
                            if let Some(body) = &mut sst.body {
                                fold_stmts(&mut body.stmts, changed);
                            }
                        }
                        _ => {}
                    },
                }
            }
        }
        E::Unary(u) => walk_expr(&mut u.arg, changed),
        E::Update(u) => walk_expr(&mut u.arg, changed),
        E::Bin(b) => {
            walk_expr(&mut b.left, changed);
            walk_expr(&mut b.right, changed);
        }
        E::Assign(a) => walk_expr(&mut a.right, changed),
        E::Member(m) => walk_expr(&mut m.obj, changed),
        E::Cond(c) => {
            walk_expr(&mut c.test, changed);
            walk_expr(&mut c.cons, changed);
            walk_expr(&mut c.alt, changed);
        }
        E::Call(c) => {
            if let ast::Callee::Expr(e) = &mut c.callee {
                walk_expr(e, changed);
            }
            for a in &mut c.args {
                walk_expr(&mut a.expr, changed);
            }
        }
        E::New(n) => {
            walk_expr(&mut n.callee, changed);
            if let Some(args) = &mut n.args {
                for a in args {
                    walk_expr(&mut a.expr, changed);
                }
            }
        }
        E::Seq(s) => {
            for e in &mut s.exprs {
                walk_expr(e, changed);
            }
        }
        E::Tpl(t) => {
            for e in &mut t.exprs {
                walk_expr(e, changed);
            }
        }
        E::Paren(p) => walk_expr(&mut p.expr, changed),
        E::Await(a) => walk_expr(&mut a.arg, changed),
        E::Yield(y) => {
            if let Some(a) = &mut y.arg {
                walk_expr(a, changed);
            }
        }
        E::TsAs(t) => walk_expr(&mut t.expr, changed),
        E::TsNonNull(t) => walk_expr(&mut t.expr, changed),
        E::TsSatisfies(t) => walk_expr(&mut t.expr, changed),
        _ => {}
    }
}
