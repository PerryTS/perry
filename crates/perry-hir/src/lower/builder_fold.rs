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
//! - #10353: the assignments need not follow the binding IMMEDIATELY. The
//!   scan skips up to `MAX_FOLD_GAP_STMTS` statements in between when
//!   moving the allocation below them is unobservable by the same argument
//!   — `gap_stmt_is_hoistable` requires exactly what the value side already
//!   requires: the statement must not name the binding, and it must not be
//!   able to execute user code. Skipped statements keep their relative
//!   order and still run before every appended value, so
//!   `const o = {}; const X = 1; o.a = X;` folds to
//!   `const X = 1; const o = { a: X };`. That gap is the ordinary shape of
//!   initialisation code, and before #10353 it cost 75×: the unfolded form
//!   leaves a 0-field anon shape that denies `Ptr<Shape>` containment, so
//!   every store takes the dynamic `PutValueSet` path. A gap is allowed only
//!   for an EMPTY literal — sinking a populated one would move its own value
//!   expressions below the skipped statements, and `const o = { a: y };
//!   const y = 1; o.b = 2;` must keep throwing on `y`'s TDZ.
//! - Values must not reference the bound name (checked conservatively by
//!   symbol name anywhere in the value expression, ignoring shadowing), so
//!   no expression can observe the half-built object.
//! - #10357: values must not run user code that could read the binding
//!   either. Calls, getters and the like never qualify. An implicit
//!   conversion (`"" + w`, `-w`, `` `${w}` ``) calls a user `valueOf` too,
//!   and a bare read of a name that is no binding reads the global object,
//!   whose property may be an accessor. Both qualify only when nothing in
//!   the builder's scope can read the binding early (`FoldScope`: no
//!   function-like that names it and can already exist, no `eval`, `with` or
//!   export, not a module-level `var`); otherwise a conversion needs operands
//!   that are primitive by construction and a read needs a proven
//!   declarative binding (`Visible`). Before #10357 both always qualified,
//!   and `const o = {}; o.a = "" + w;` threw a TDZ `ReferenceError` whenever
//!   `w.valueOf` read `o`.
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
//! - A module that mutates `Object.prototype` through the standard descriptor
//!   APIs is excluded. An inherited setter or non-writable data descriptor
//!   makes `o.k = v` observably different from `{ k: v }`: the former performs
//!   `[[Set]]`, while the latter defines an own data property. Runtime write
//!   guards cannot protect a fold because the assignment no longer exists in
//!   HIR (#9034).
//! - Only `Pat::Ident` bindings qualify; the declarator may carry any type
//!   annotation. Exported declarations are skipped (scope kept tight).
//!
//! A miss here is only a missed optimization: unmatched shapes lower
//! exactly as before.

use swc_common::{BytePos, Spanned};
use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

/// Fold cap per literal — beyond this the object is dictionary-like and the
/// literal machinery's inline-slot benefits taper off anyway.
const MAX_FOLDED_PROPS: usize = 64;

/// How many statements the scan may skip between the binding and its first
/// assignment (#10353). Real builders separate the two by a handful of
/// constant bindings at most; the cap keeps the forward scan O(n) over a
/// statement list instead of O(n²) on a long run of hoistable declarations.
const MAX_FOLD_GAP_STMTS: usize = 64;

/// Returns a folded clone when at least one builder sequence was folded;
/// `None` means "nothing to do — lower the original".
pub(crate) fn fold_builder_sequences(module: &ast::Module) -> Option<ast::Module> {
    if !module_has_candidate(module) || module_mutates_object_prototype_descriptors(module) {
        return None;
    }
    let scope = FoldScope::of_module(module);
    let visible = Visible::of_module(module);
    let mut folded = module.clone();
    let mut changed = false;
    process_module_items(&mut folded.body, &mut changed, &scope, &visible);
    changed.then_some(folded)
}

/// Whether this module can make assignment to a fresh ordinary object invoke
/// inherited descriptor semantics on the canonical `Object.prototype`.
///
/// This is deliberately module-wide and conservative. Descriptor mutation is
/// rare, while trying to prove source ordering across nested closures and
/// static-import execution would be brittle. A false positive only preserves
/// the original dynamic writes; a false negative changes JavaScript behavior.
fn module_mutates_object_prototype_descriptors(module: &ast::Module) -> bool {
    struct Finder {
        found: bool,
    }

    impl Visit for Finder {
        fn visit_call_expr(&mut self, call: &ast::CallExpr) {
            if self.found {
                return;
            }
            if call_mutates_object_prototype_descriptors(call) {
                self.found = true;
                return;
            }
            call.visit_children_with(self);
        }
    }

    let mut finder = Finder { found: false };
    module.visit_with(&mut finder);
    finder.found
}

fn call_mutates_object_prototype_descriptors(call: &ast::CallExpr) -> bool {
    let ast::Callee::Expr(callee) = &call.callee else {
        return false;
    };
    let ast::Expr::Member(member) = strip_transparent_expr(callee) else {
        return false;
    };
    let Some(method) = static_member_name(member) else {
        return false;
    };

    // Legacy Annex-B mutation APIs operate directly on Object.prototype.
    if matches!(method, "__defineGetter__" | "__defineSetter__")
        && is_object_prototype_expr(&member.obj)
    {
        return true;
    }

    let descriptor_api = (is_ident_expr(&member.obj, "Object")
        && matches!(method, "defineProperty" | "defineProperties"))
        || (is_ident_expr(&member.obj, "Reflect") && method == "defineProperty");
    descriptor_api
        && call
            .args
            .first()
            .is_some_and(|arg| arg.spread.is_none() && is_object_prototype_expr(&arg.expr))
}

fn strip_transparent_expr(mut expr: &ast::Expr) -> &ast::Expr {
    loop {
        expr = match expr {
            ast::Expr::Paren(v) => &v.expr,
            ast::Expr::TsAs(v) => &v.expr,
            ast::Expr::TsNonNull(v) => &v.expr,
            ast::Expr::TsTypeAssertion(v) => &v.expr,
            ast::Expr::TsConstAssertion(v) => &v.expr,
            ast::Expr::TsSatisfies(v) => &v.expr,
            _ => return expr,
        };
    }
}

fn static_member_name(member: &ast::MemberExpr) -> Option<&str> {
    match &member.prop {
        ast::MemberProp::Ident(name) => Some(name.sym.as_ref()),
        ast::MemberProp::Computed(key) => match strip_transparent_expr(&key.expr) {
            ast::Expr::Lit(ast::Lit::Str(value)) => value.value.as_str(),
            _ => None,
        },
        ast::MemberProp::PrivateName(_) => None,
    }
}

fn is_ident_expr(expr: &ast::Expr, expected: &str) -> bool {
    matches!(strip_transparent_expr(expr), ast::Expr::Ident(id) if id.sym.as_ref() == expected)
}

fn is_object_prototype_expr(expr: &ast::Expr) -> bool {
    let ast::Expr::Member(member) = strip_transparent_expr(expr) else {
        return false;
    };
    static_member_name(member) == Some("prototype") && is_ident_expr(&member.obj, "Object")
}

/// Cheap read-only pre-scan: is any statement list anywhere (including
/// function bodies nested in expressions) a `const/let/var x = {…}`
/// followed — across a hoistable gap (#10353) — by a static member
/// assignment to the same name? False positives only cost the clone; a
/// false negative would skip a fold, so the walk mirrors the mutating
/// one's reach, gap included.
fn module_has_candidate(module: &ast::Module) -> bool {
    for (i, item) in module.body.iter().enumerate() {
        let ast::ModuleItem::Stmt(a) = item else {
            continue;
        };
        let (Some(name), _) = decl_object_binding(a) else {
            continue;
        };
        let item_stmt = |k: usize| match module.body.get(i + 1 + k) {
            Some(ast::ModuleItem::Stmt(s)) => Some(s),
            _ => None,
        };
        let gap = fold_gap_len(name.as_str(), false, &Visible::ROOT, item_stmt);
        if item_stmt(gap).is_some_and(|b| assign_to_name_key(b, name.as_str()).is_some()) {
            return true;
        }
    }
    module.body.iter().any(|item| match item {
        ast::ModuleItem::Stmt(s) => scan_stmt(s),
        ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(ed)) => scan_decl(&ed.decl),
        ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDefaultExpr(e)) => scan_expr(&e.expr),
        _ => false,
    })
}

fn stmts_have_candidate(stmts: &[ast::Stmt]) -> bool {
    for (i, s) in stmts.iter().enumerate() {
        let (Some(name), _) = decl_object_binding(s) else {
            continue;
        };
        let gap = fold_gap_len(name.as_str(), false, &Visible::ROOT, |k| {
            stmts.get(i + 1 + k)
        });
        if stmts
            .get(i + 1 + gap)
            .is_some_and(|b| assign_to_name_key(b, name.as_str()).is_some())
        {
            return true;
        }
    }
    stmts.iter().any(scan_stmt)
}

fn scan_stmt(s: &ast::Stmt) -> bool {
    match s {
        ast::Stmt::Block(b) => stmts_have_candidate(&b.stmts),
        ast::Stmt::If(i) => {
            scan_expr(&i.test) || scan_stmt(&i.cons) || i.alt.as_deref().is_some_and(scan_stmt)
        }
        ast::Stmt::While(w) => scan_expr(&w.test) || scan_stmt(&w.body),
        ast::Stmt::DoWhile(d) => scan_stmt(&d.body) || scan_expr(&d.test),
        ast::Stmt::For(f) => {
            matches!(&f.init, Some(ast::VarDeclOrExpr::Expr(e)) if scan_expr(e))
                || f.test.as_deref().is_some_and(scan_expr)
                || f.update.as_deref().is_some_and(scan_expr)
                || scan_stmt(&f.body)
        }
        ast::Stmt::ForIn(f) => scan_stmt(&f.body),
        ast::Stmt::ForOf(f) => scan_stmt(&f.body),
        ast::Stmt::Labeled(l) => scan_stmt(&l.body),
        ast::Stmt::Try(t) => {
            stmts_have_candidate(&t.block.stmts)
                || t.handler
                    .as_ref()
                    .is_some_and(|h| stmts_have_candidate(&h.body.stmts))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| stmts_have_candidate(&f.stmts))
        }
        ast::Stmt::Switch(sw) => {
            scan_expr(&sw.discriminant) || sw.cases.iter().any(|c| stmts_have_candidate(&c.cons))
        }
        ast::Stmt::Decl(d) => scan_decl(d),
        ast::Stmt::Expr(es) => scan_expr(&es.expr),
        ast::Stmt::Return(r) => r.arg.as_deref().is_some_and(scan_expr),
        ast::Stmt::Throw(t) => scan_expr(&t.arg),
        _ => false,
    }
}

fn scan_decl(d: &ast::Decl) -> bool {
    match d {
        ast::Decl::Fn(f) => f
            .function
            .body
            .as_ref()
            .is_some_and(|b| stmts_have_candidate(&b.stmts)),
        ast::Decl::Class(c) => scan_class(&c.class),
        ast::Decl::Var(v) => v
            .decls
            .iter()
            .any(|d| d.init.as_deref().is_some_and(scan_expr)),
        _ => false,
    }
}

fn scan_class(class: &ast::Class) -> bool {
    class.body.iter().any(|m| match m {
        ast::ClassMember::Method(m) => m
            .function
            .body
            .as_ref()
            .is_some_and(|b| stmts_have_candidate(&b.stmts)),
        ast::ClassMember::PrivateMethod(m) => m
            .function
            .body
            .as_ref()
            .is_some_and(|b| stmts_have_candidate(&b.stmts)),
        ast::ClassMember::Constructor(c) => c
            .body
            .as_ref()
            .is_some_and(|b| stmts_have_candidate(&b.stmts)),
        ast::ClassMember::StaticBlock(b) => stmts_have_candidate(&b.body.stmts),
        ast::ClassMember::ClassProp(p) => p.value.as_deref().is_some_and(scan_expr),
        ast::ClassMember::PrivateProp(p) => p.value.as_deref().is_some_and(scan_expr),
        _ => false,
    })
}

fn scan_expr(e: &ast::Expr) -> bool {
    use ast::Expr as E;
    match e {
        E::Fn(f) => f
            .function
            .body
            .as_ref()
            .is_some_and(|b| stmts_have_candidate(&b.stmts)),
        E::Arrow(a) => match &*a.body {
            ast::BlockStmtOrExpr::BlockStmt(b) => stmts_have_candidate(&b.stmts),
            ast::BlockStmtOrExpr::Expr(e) => scan_expr(e),
        },
        E::Class(c) => scan_class(&c.class),
        E::Array(a) => a.elems.iter().flatten().any(|el| scan_expr(&el.expr)),
        E::Object(o) => o.props.iter().any(|p| match p {
            ast::PropOrSpread::Spread(sp) => scan_expr(&sp.expr),
            ast::PropOrSpread::Prop(prop) => match &**prop {
                ast::Prop::KeyValue(kv) => scan_expr(&kv.value),
                ast::Prop::Method(m) => m
                    .function
                    .body
                    .as_ref()
                    .is_some_and(|b| stmts_have_candidate(&b.stmts)),
                ast::Prop::Getter(g) => g
                    .body
                    .as_ref()
                    .is_some_and(|b| stmts_have_candidate(&b.stmts)),
                ast::Prop::Setter(st) => st
                    .body
                    .as_ref()
                    .is_some_and(|b| stmts_have_candidate(&b.stmts)),
                _ => false,
            },
        }),
        E::Unary(u) => scan_expr(&u.arg),
        E::Update(u) => scan_expr(&u.arg),
        E::Bin(b) => scan_expr(&b.left) || scan_expr(&b.right),
        E::Assign(a) => scan_expr(&a.right),
        E::Member(m) => scan_expr(&m.obj),
        E::Cond(c) => scan_expr(&c.test) || scan_expr(&c.cons) || scan_expr(&c.alt),
        E::Call(c) => {
            matches!(&c.callee, ast::Callee::Expr(e) if scan_expr(e))
                || c.args.iter().any(|a| scan_expr(&a.expr))
        }
        E::New(n) => scan_expr(&n.callee) || n.args.iter().flatten().any(|a| scan_expr(&a.expr)),
        E::Seq(s) => s.exprs.iter().any(|e| scan_expr(e)),
        E::Tpl(t) => t.exprs.iter().any(|e| scan_expr(e)),
        E::Paren(p) => scan_expr(&p.expr),
        E::Await(a) => scan_expr(&a.arg),
        E::Yield(y) => y.arg.as_deref().is_some_and(scan_expr),
        E::TsAs(t) => scan_expr(&t.expr),
        E::TsNonNull(t) => scan_expr(&t.expr),
        E::TsSatisfies(t) => scan_expr(&t.expr),
        _ => false,
    }
}

fn process_module_items(
    items: &mut [ast::ModuleItem],
    changed: &mut bool,
    scope: &FoldScope,
    visible: &Visible<'_>,
) {
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
            fold_module_stmt_run(&mut items[i..j], changed, scope, visible);
            for item in items[i..j].iter_mut() {
                if let ast::ModuleItem::Stmt(s) = item {
                    walk_stmt(s, changed, scope, visible);
                }
            }
            i = j;
        } else {
            if let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(ed)) = &mut items[i] {
                walk_decl(&mut ed.decl, changed, visible);
            }
            i += 1;
        }
    }
}

/// Fold within a run of top-level ModuleItem::Stmt entries. Consumed
/// assignment statements are replaced with `;` (EmptyStmt).
fn fold_module_stmt_run(
    items: &mut [ast::ModuleItem],
    changed: &mut bool,
    scope: &FoldScope,
    visible: &Visible<'_>,
) {
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
        let observable = match &items[idx] {
            ast::ModuleItem::Stmt(s) => scope.observes(&name_start, s),
            _ => true,
        };
        // A gap is only skippable for an EMPTY literal: sinking a populated
        // one would move its own value expressions below the skipped
        // statements, and `const o = { a: y }; const y = 1; o.b = 2;` must
        // keep throwing on `y`'s TDZ.
        let gap = if existing.is_empty() {
            fold_gap_len(&name_start, observable, visible, |k| {
                match items.get(idx + 1 + k) {
                    Some(ast::ModuleItem::Stmt(s)) => Some(s),
                    _ => None,
                }
            })
        } else {
            0
        };
        let first = idx + 1 + gap;
        let mut keys = existing_keys(existing);
        let mut appended: Vec<(ast::PropName, Box<ast::Expr>)> = Vec::new();
        let mut consumed = 0usize;
        for follower in items[first..].iter() {
            let ast::ModuleItem::Stmt(fs) = follower else {
                break;
            };
            let Some((key, value)) = assign_to_name_key(fs, &name_start) else {
                break;
            };
            if !fold_key_ok(&key, &keys)
                || !value_is_fold_safe(value, &name_start, observable, visible)
            {
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
        // Apply: extend the literal, sink the declaration below the skipped
        // statements so the appended values still evaluate after them, and
        // blank out the consumed statements.
        if let ast::ModuleItem::Stmt(s) = &mut items[idx] {
            append_props(s, appended);
        }
        if gap > 0 {
            items[idx..first].rotate_left(1);
        }
        for follower in items[first..first + consumed].iter_mut() {
            *follower = ast::ModuleItem::Stmt(ast::Stmt::Empty(ast::EmptyStmt {
                span: swc_common::DUMMY_SP,
            }));
        }
        *changed = true;
        if gap > 0 {
            // `items[idx]` is now the first skipped statement, which may open
            // a builder of its own (`const a = {}; const b = {}; a.x = 1;
            // b.y = 2;`). Re-examining it terminates: each fold blanks at
            // least one assignment statement, and the run holds finitely many.
            continue;
        }
        idx += 1 + consumed;
    }
}

fn fold_stmts(
    stmts: &mut Vec<ast::Stmt>,
    changed: &mut bool,
    scope: &FoldScope,
    visible: &Visible<'_>,
) {
    // A list's own declarations are bindings for the whole list.
    let visible = &visible.child(declared_names(stmts));
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
        let observable = scope.observes(&name, &stmts[idx]);
        // Empty literals only — see `fold_module_stmt_run`.
        let gap = if existing_len == 0 {
            fold_gap_len(&name, observable, visible, |k| stmts.get(idx + 1 + k))
        } else {
            0
        };
        let first = idx + 1 + gap;
        let mut appended: Vec<(ast::PropName, Box<ast::Expr>)> = Vec::new();
        let mut consumed = 0usize;
        for follower in stmts[first..].iter() {
            let Some((key, value)) = assign_to_name_key(follower, &name) else {
                break;
            };
            if !fold_key_ok(&key, &keys) || !value_is_fold_safe(value, &name, observable, visible) {
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
            // Sink the declaration below the skipped statements: the appended
            // values evaluate where the literal now sits, so they must still
            // run after everything that used to precede them.
            if gap > 0 {
                stmts[idx..first].rotate_left(1);
            }
            stmts.drain(first..first + consumed);
            *changed = true;
            if gap > 0 {
                // `stmts[idx]` is now the first skipped statement, which may
                // open a builder of its own. Re-examining it terminates: each
                // fold removes at least one statement from the list.
                continue;
            }
        }
        idx += 1;
    }
    for s in stmts.iter_mut() {
        walk_stmt(s, changed, scope, visible);
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

/// How many statements between the `{ … }` binding and its first fold-able
/// assignment the scan may skip (#10353).
///
/// `at(k)` yields the k-th follower of the binding, or `None` when the run
/// ends (a non-`Stmt` module item, or the end of the list). The walk stops at
/// the first statement that is an assignment to `name` — that is where the
/// fold proper takes over — and at the first statement the declaration may
/// not move below.
fn fold_gap_len<'a>(
    name: &str,
    observable: bool,
    visible: &Visible<'_>,
    at: impl Fn(usize) -> Option<&'a ast::Stmt>,
) -> usize {
    let mut gap = 0usize;
    while gap < MAX_FOLD_GAP_STMTS {
        let Some(s) = at(gap) else { break };
        if assign_to_name_key(s, name).is_some()
            || !gap_stmt_is_hoistable(s, name, observable, visible)
        {
            break;
        }
        gap += 1;
    }
    gap
}

/// May the `{ … }` declaration move BELOW this statement?
///
/// The fold evaluates the appended values where the literal ends up, so a
/// statement standing between the binding and its first assignment is only
/// skippable when moving the ALLOCATION past it is unobservable. That is the
/// same pair of conditions `value_is_fold_safe` already enforces on the value
/// side, for the same two reasons:
///
/// - the statement must not NAME the binding — it would otherwise read, write
///   or capture an object that no longer exists at that point (`const o = {};
///   f(o); o.a = 1` keeps its dynamic writes);
/// - the statement must not be able to execute user code, because a call can
///   reach a hoisted `function peek() { return o; }` that names the binding
///   WITHOUT the statement naming it, turning a successful read into a TDZ
///   `ReferenceError`. Reusing `value_is_fold_safe` for initializers and
///   expression statements buys exactly that test — including its #10357
///   rule for implicit conversions, which take the same builder's
///   `observable` answer so the two sides cannot drift apart.
///
/// Destructuring patterns are excluded: the binding itself performs property
/// reads, which can run a getter. Type-only declarations are erased before
/// codegen, so they carry no runtime effect at all and always qualify —
/// `enum` and `namespace` do emit code and do not.
fn gap_stmt_is_hoistable(
    s: &ast::Stmt,
    name: &str,
    observable: bool,
    visible: &Visible<'_>,
) -> bool {
    match s {
        ast::Stmt::Empty(_) => true,
        ast::Stmt::Decl(ast::Decl::TsInterface(_) | ast::Decl::TsTypeAlias(_)) => true,
        ast::Stmt::Expr(es) => value_is_fold_safe(&es.expr, name, observable, visible),
        ast::Stmt::Decl(ast::Decl::Var(var)) => var.decls.iter().all(|d| {
            matches!(&d.name, ast::Pat::Ident(bi) if bi.id.sym.as_ref() != name)
                && d.init
                    .as_deref()
                    .is_none_or(|init| value_is_fold_safe(init, name, observable, visible))
        }),
        _ => false,
    }
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

/// May this VALUE expression fold into a literal that now evaluates it
/// BEFORE the builder binding is initialized?
///
/// A call, `new`, member read (getters), optional chain, tagged template,
/// spread (iterator protocols), `in`/`instanceof` (traps /
/// `Symbol.hasInstance`), `await`/`yield`, or any function-bearing form can
/// run arbitrary user code, so none of them ever qualifies — that code could
/// reach the binding through a closure WITHOUT the value naming it (e.g. a
/// hoisted `function f() { return o.a; }` observed via `o.b = f()` — folding
/// would turn the original's successful read into a TDZ ReferenceError).
/// Reading OTHER identifiers is safe (identical evaluation either side of
/// the allocation); reading the builder's own name is excluded directly.
///
/// #10357: an implicit conversion runs user code too. `"" + w`, `-w`, `w < 1`
/// and `` `${w}` `` call `w`'s `valueOf`/`toString`/`Symbol.toPrimitive`. That
/// is only a problem when such code can read the binding at all, which is
/// what `observable` answers (`FoldScope::observes`): when nothing in scope
/// can, the conversion is unobservable and folds as before; when something
/// can, a converting operator qualifies only on operands that are primitive
/// by construction (`is_primitive_valued`), where no user code runs.
///
/// A bare identifier read is the same hazard in disguise: one that resolves
/// to no binding reads a property of the global object, and that property
/// can be an accessor whose getter reads the builder. On an observable
/// builder a read therefore qualifies only when `visible` proves it resolves
/// to a declarative binding.
fn value_is_fold_safe(e: &ast::Expr, name: &str, observable: bool, visible: &Visible<'_>) -> bool {
    use ast::Expr as E;
    let safe = |x: &ast::Expr| value_is_fold_safe(x, name, observable, visible);
    let read_ok = |sym: &str| sym != name && (!observable || visible.resolves(sym));
    match e {
        E::Lit(_) | E::This(_) => true,
        E::Ident(i) => read_ok(i.sym.as_ref()),
        E::Paren(p) => safe(&p.expr),
        E::Tpl(t) => t
            .exprs
            .iter()
            .all(|x| safe(x) && (!observable || is_primitive_valued(x))),
        E::Unary(u) => {
            u.op != ast::UnaryOp::Delete
                && safe(&u.arg)
                && (!observable || !unary_converts(u.op) || is_primitive_valued(&u.arg))
        }
        E::Bin(b) => {
            !matches!(b.op, ast::BinaryOp::In | ast::BinaryOp::InstanceOf)
                && safe(&b.left)
                && safe(&b.right)
                && (!observable
                    || !binary_converts(b.op)
                    || (is_primitive_valued(&b.left) && is_primitive_valued(&b.right)))
        }
        E::Cond(c) => safe(&c.test) && safe(&c.cons) && safe(&c.alt),
        E::Seq(sq) => sq.exprs.iter().all(|x| safe(x)),
        E::Array(a) => a.elems.iter().all(|el| match el {
            None => true,
            Some(el) => el.spread.is_none() && safe(&el.expr),
        }),
        E::Object(o) => o.props.iter().all(|p| match p {
            ast::PropOrSpread::Spread(_) => false,
            ast::PropOrSpread::Prop(prop) => match &**prop {
                ast::Prop::KeyValue(kv) => {
                    matches!(kv.key, ast::PropName::Ident(_) | ast::PropName::Str(_))
                        && safe(&kv.value)
                }
                ast::Prop::Shorthand(i) => read_ok(i.sym.as_ref()),
                _ => false,
            },
        }),
        E::TsAs(t) => safe(&t.expr),
        E::TsNonNull(t) => safe(&t.expr),
        E::TsTypeAssertion(t) => safe(&t.expr),
        E::TsSatisfies(t) => safe(&t.expr),
        E::TsConstAssertion(t) => safe(&t.expr),
        // Everything else — calls, news, member/optional access, tagged
        // templates, await/yield, updates, assignments, function-bearing
        // forms, unknown variants — may execute user code: unsafe to hoist
        // past the allocation.
        _ => false,
    }
}

/// Does this unary operator convert its operand (`ToNumeric`)? `!` is
/// `ToBoolean`, `typeof`/`void` inspect without converting — none of those
/// can call user code.
fn unary_converts(op: ast::UnaryOp) -> bool {
    matches!(
        op,
        ast::UnaryOp::Minus | ast::UnaryOp::Plus | ast::UnaryOp::Tilde
    )
}

/// Does this binary operator convert its operands? Only strict equality and
/// the short-circuiting operators (`ToBoolean`, or no conversion at all)
/// cannot reach `valueOf`/`toString`. Loose equality can — an object
/// compared with a primitive goes through `ToPrimitive`.
fn binary_converts(op: ast::BinaryOp) -> bool {
    !matches!(
        op,
        ast::BinaryOp::EqEqEq
            | ast::BinaryOp::NotEqEq
            | ast::BinaryOp::LogicalAnd
            | ast::BinaryOp::LogicalOr
            | ast::BinaryOp::NullishCoalescing
    )
}

/// Is this expression's value a primitive by construction, whatever its
/// operands are? A conversion of a primitive runs no user code. Deliberately
/// syntactic: identifiers are never primitive here (no type facts at this
/// stage, and a `: number` annotation is not a proof), and a regex literal is
/// an object whose `toString` is user-patchable.
fn is_primitive_valued(e: &ast::Expr) -> bool {
    use ast::Expr as E;
    match e {
        E::Lit(lit) => !matches!(lit, ast::Lit::Regex(_) | ast::Lit::JSXText(_)),
        // A template is a string and every unary operator yields a
        // primitive, whatever their operands.
        E::Tpl(_) | E::Unary(_) => true,
        E::Bin(b) => match b.op {
            // These return one of their operands.
            ast::BinaryOp::LogicalAnd
            | ast::BinaryOp::LogicalOr
            | ast::BinaryOp::NullishCoalescing => {
                is_primitive_valued(&b.left) && is_primitive_valued(&b.right)
            }
            _ => true,
        },
        E::Cond(c) => is_primitive_valued(&c.cons) && is_primitive_valued(&c.alt),
        E::Seq(sq) => sq.exprs.last().is_some_and(|x| is_primitive_valued(x)),
        E::Paren(p) => is_primitive_valued(&p.expr),
        E::TsAs(t) => is_primitive_valued(&t.expr),
        E::TsNonNull(t) => is_primitive_valued(&t.expr),
        E::TsTypeAssertion(t) => is_primitive_valued(&t.expr),
        E::TsSatisfies(t) => is_primitive_valued(&t.expr),
        E::TsConstAssertion(t) => is_primitive_valued(&t.expr),
        _ => false,
    }
}

/// Which builders in one var scope (a function-like body, or the module)
/// could be read by user code before their folded literal initializes them
/// (#10357).
///
/// A binding can only be read by code that NAMES it, so the user code a
/// conversion reaches must be a function-like nested in the binding's own
/// scope whose body mentions the name. Three refinements keep that precise,
/// because every false positive costs a fold:
///
/// - **Position.** A `let`/`const` binding is fresh on every pass through an
///   enclosing loop, and execution within one pass only moves forward, so a
///   function-like CREATED after the declaration cannot run before it. A
///   hoisted function declaration exists from scope entry and always counts.
///   A `var` binding is shared by every pass, so a closure made later in one
///   pass can run during the next pass's fold: for a `var`, any mention
///   counts.
/// - **Shadowing.** A nested function-like that re-binds the name — as a
///   parameter, or a declaration at the top level of its body — reads its own
///   binding, not the builder. Body declarations do not shadow parameter
///   defaults, which are evaluated in a scope of their own; block-level
///   re-declarations are not tracked and so conservatively still count.
/// - **Escapes the name scan cannot see** make every builder observable:
///   `eval` anywhere in the scope (Perry compiles a literal `eval("o")` into a
///   closure that reads `o`), `with` (identifier resolution becomes property
///   lookup), an export of the name (an importer reads the live binding), and
///   a module-level `var` (a global-object property).
///
/// The scope is the whole enclosing function (or module), not the statement
/// list being folded: a `var` is function-scoped, and a `let` in one `case`
/// is visible to every other case of its `switch`.
#[derive(Default)]
struct FoldScope {
    observers: Vec<Observer>,
    /// `eval` or `with` inside the scope.
    opaque: bool,
    /// Module top level.
    top_level: bool,
}

/// The function-likes that name one builder.
struct Observer {
    name: String,
    /// Named from code that exists before the scope runs anything: a hoisted
    /// function declaration, or an export.
    always: bool,
    /// Where the earliest other function-like naming it starts.
    first_lo: Option<BytePos>,
}

impl FoldScope {
    fn of_module(module: &ast::Module) -> Self {
        let mut scope = Self::scan(|v| module.visit_with(v));
        scope.top_level = true;
        scope
    }

    fn of_body(stmts: &[ast::Stmt]) -> Self {
        Self::scan(|v| stmts.visit_with(v))
    }

    /// Two passes, and the second only when it can change an answer: the
    /// scope must bind an object literal (a builder candidate) at its own
    /// depth.
    fn scan(visit: impl Fn(&mut dyn Visit)) -> Self {
        let mut own = OwnScopeScan::default();
        visit(&mut own);
        if own.builders.is_empty() {
            return Self::default();
        }
        let mut observers = ObserverScan::new(&own.builders);
        visit(&mut observers);
        Self {
            observers: observers.observers,
            opaque: observers.opaque,
            top_level: false,
        }
    }

    /// Is the builder declared by `decl` (bound to `name`) observable?
    fn observes(&self, name: &str, decl: &ast::Stmt) -> bool {
        let is_var = matches!(
            decl,
            ast::Stmt::Decl(ast::Decl::Var(v)) if v.kind == ast::VarDeclKind::Var
        );
        if self.opaque || (self.top_level && is_var) {
            return true;
        }
        let Some(observer) = self.observers.iter().find(|o| o.name == name) else {
            return false;
        };
        observer.always || is_var || observer.first_lo.is_some_and(|lo| lo < decl.span().hi)
    }
}

/// Pass 1: the scope's OWN depth — never enters a nested function-like.
/// Collects object-literal binding names.
#[derive(Default)]
struct OwnScopeScan {
    builders: Vec<String>,
}

impl Visit for OwnScopeScan {
    fn visit_var_declarator(&mut self, d: &ast::VarDeclarator) {
        if let (ast::Pat::Ident(bi), Some(init)) = (&d.name, d.init.as_deref()) {
            if matches!(init, ast::Expr::Object(_)) {
                let name = bi.id.sym.to_string();
                if !self.builders.contains(&name) {
                    self.builders.push(name);
                }
            }
        }
        d.visit_children_with(self);
    }
    // Nested function-likes are separate scopes with their own scan.
    fn visit_function(&mut self, _: &ast::Function) {}
    fn visit_arrow_expr(&mut self, _: &ast::ArrowExpr) {}
    fn visit_constructor(&mut self, _: &ast::Constructor) {}
    fn visit_getter_prop(&mut self, _: &ast::GetterProp) {}
    fn visit_setter_prop(&mut self, _: &ast::SetterProp) {}
    fn visit_class_prop(&mut self, _: &ast::ClassProp) {}
    fn visit_private_prop(&mut self, _: &ast::PrivateProp) {}
    fn visit_auto_accessor(&mut self, _: &ast::AutoAccessor) {}
    fn visit_static_block(&mut self, _: &ast::StaticBlock) {}
}

/// Pass 2: every depth. Records where each builder name is mentioned by a
/// nested function-like or an export, and any `eval`/`with`.
struct ObserverScan<'b> {
    builders: &'b [String],
    depth: u32,
    in_export: bool,
    /// Set by a function declaration for the function it declares.
    declared: bool,
    /// The outermost nested function-like being walked.
    frame_hoisted: bool,
    frame_lo: BytePos,
    /// Builder names re-bound by an enclosing nested function-like.
    shadowed: Vec<String>,
    observers: Vec<Observer>,
    opaque: bool,
}

impl<'b> ObserverScan<'b> {
    fn new(builders: &'b [String]) -> Self {
        Self {
            builders,
            depth: 0,
            in_export: false,
            declared: false,
            frame_hoisted: false,
            frame_lo: BytePos::DUMMY,
            shadowed: Vec::new(),
            observers: Vec::new(),
            opaque: false,
        }
    }

    fn enter(&mut self, lo: BytePos) -> usize {
        let declared = std::mem::take(&mut self.declared);
        if self.depth == 0 {
            self.frame_hoisted = declared;
            self.frame_lo = lo;
        }
        self.depth += 1;
        self.shadowed.len()
    }

    fn leave(&mut self, mark: usize) {
        self.depth -= 1;
        self.shadowed.truncate(mark);
    }

    fn bind(&mut self, sym: &str) {
        if self.builders.iter().any(|b| b == sym) {
            self.shadowed.push(sym.to_string());
        }
    }

    fn bind_pat(&mut self, pat: &ast::Pat) {
        if let ast::Pat::Ident(bi) = pat {
            self.bind(bi.id.sym.as_ref());
        }
    }

    /// Declarations at the top level of a function-like body bind for the
    /// whole body.
    fn bind_body(&mut self, stmts: &[ast::Stmt]) {
        for stmt in stmts {
            match stmt {
                ast::Stmt::Decl(ast::Decl::Var(v)) => {
                    for d in &v.decls {
                        self.bind_pat(&d.name);
                    }
                }
                ast::Stmt::Decl(ast::Decl::Fn(f)) => self.bind(f.ident.sym.as_ref()),
                ast::Stmt::Decl(ast::Decl::Class(c)) => self.bind(c.ident.sym.as_ref()),
                _ => {}
            }
        }
    }

    fn record(&mut self, sym: &str) {
        if !self.builders.iter().any(|b| b == sym) || self.shadowed.iter().any(|s| s == sym) {
            return;
        }
        let always = self.in_export || self.frame_hoisted;
        let lo = self.frame_lo;
        match self.observers.iter_mut().find(|o| o.name == sym) {
            Some(observer) => {
                observer.always |= always;
                if !always {
                    observer.first_lo = Some(observer.first_lo.map_or(lo, |first| first.min(lo)));
                }
            }
            None => self.observers.push(Observer {
                name: sym.to_string(),
                always,
                first_lo: (!always).then_some(lo),
            }),
        }
    }
}

impl Visit for ObserverScan<'_> {
    fn visit_ident(&mut self, i: &ast::Ident) {
        let sym = i.sym.as_ref();
        if sym == "eval" {
            self.opaque = true;
        }
        if self.depth > 0 || self.in_export {
            self.record(sym);
        }
    }
    fn visit_with_stmt(&mut self, w: &ast::WithStmt) {
        self.opaque = true;
        w.visit_children_with(self);
    }
    fn visit_module_decl(&mut self, d: &ast::ModuleDecl) {
        let outer = std::mem::replace(&mut self.in_export, true);
        d.visit_children_with(self);
        self.in_export = outer;
    }
    fn visit_fn_decl(&mut self, f: &ast::FnDecl) {
        // The declared name is a binding, not a read.
        self.declared = true;
        f.function.visit_with(self);
    }
    fn visit_fn_expr(&mut self, f: &ast::FnExpr) {
        // A named function expression binds its own name inside itself.
        let mark = self.shadowed.len();
        if let Some(ident) = &f.ident {
            self.bind(ident.sym.as_ref());
        }
        f.function.visit_with(self);
        self.shadowed.truncate(mark);
    }
    fn visit_function(&mut self, f: &ast::Function) {
        let mark = self.enter(f.span.lo);
        for param in &f.params {
            self.bind_pat(&param.pat);
        }
        f.decorators.visit_with(self);
        f.params.visit_with(self);
        if let Some(body) = &f.body {
            self.bind_body(&body.stmts);
            body.visit_with(self);
        }
        self.leave(mark);
    }
    fn visit_arrow_expr(&mut self, a: &ast::ArrowExpr) {
        let mark = self.enter(a.span.lo);
        for param in &a.params {
            self.bind_pat(param);
        }
        a.params.visit_with(self);
        match &*a.body {
            ast::BlockStmtOrExpr::BlockStmt(body) => {
                self.bind_body(&body.stmts);
                body.visit_with(self);
            }
            ast::BlockStmtOrExpr::Expr(body) => body.visit_with(self),
        }
        self.leave(mark);
    }
    fn visit_constructor(&mut self, c: &ast::Constructor) {
        let mark = self.enter(c.span.lo);
        for param in &c.params {
            match param {
                ast::ParamOrTsParamProp::Param(p) => self.bind_pat(&p.pat),
                ast::ParamOrTsParamProp::TsParamProp(p) => {
                    if let ast::TsParamPropParam::Ident(bi) = &p.param {
                        self.bind(bi.id.sym.as_ref());
                    }
                }
            }
        }
        c.key.visit_with(self);
        c.params.visit_with(self);
        if let Some(body) = &c.body {
            self.bind_body(&body.stmts);
            body.visit_with(self);
        }
        self.leave(mark);
    }
    fn visit_getter_prop(&mut self, g: &ast::GetterProp) {
        let mark = self.enter(g.span.lo);
        g.key.visit_with(self);
        if let Some(body) = &g.body {
            self.bind_body(&body.stmts);
            body.visit_with(self);
        }
        self.leave(mark);
    }
    fn visit_setter_prop(&mut self, st: &ast::SetterProp) {
        let mark = self.enter(st.span.lo);
        self.bind_pat(&st.param);
        st.key.visit_with(self);
        st.param.visit_with(self);
        if let Some(body) = &st.body {
            self.bind_body(&body.stmts);
            body.visit_with(self);
        }
        self.leave(mark);
    }
    fn visit_class_prop(&mut self, p: &ast::ClassProp) {
        let mark = self.enter(p.span.lo);
        p.visit_children_with(self);
        self.leave(mark);
    }
    fn visit_private_prop(&mut self, p: &ast::PrivateProp) {
        let mark = self.enter(p.span.lo);
        p.visit_children_with(self);
        self.leave(mark);
    }
    fn visit_auto_accessor(&mut self, a: &ast::AutoAccessor) {
        let mark = self.enter(a.span.lo);
        a.visit_children_with(self);
        self.leave(mark);
    }
    fn visit_static_block(&mut self, b: &ast::StaticBlock) {
        let mark = self.enter(b.span.lo);
        self.bind_body(&b.body.stmts);
        b.body.visit_with(self);
        self.leave(mark);
    }
}

fn walk_decl(d: &mut ast::Decl, changed: &mut bool, visible: &Visible<'_>) {
    match d {
        ast::Decl::Fn(f) => {
            let params = param_names(f.function.params.iter().map(|p| &p.pat));
            if let Some(body) = &mut f.function.body {
                fold_body(&mut body.stmts, changed, params, visible);
            }
        }
        ast::Decl::Class(c) => walk_class(&mut c.class, changed, visible),
        ast::Decl::Var(v) => {
            for decl in &mut v.decls {
                if let Some(init) = &mut decl.init {
                    walk_expr(init, changed, visible);
                }
            }
        }
        _ => {}
    }
}

/// Fold a function-like body: its own var scope, so its own `FoldScope`, and
/// its parameters and function-scoped `var`s as bindings for all of it.
fn fold_body(
    stmts: &mut Vec<ast::Stmt>,
    changed: &mut bool,
    mut bindings: Vec<String>,
    parent: &Visible<'_>,
) {
    let scope = FoldScope::of_body(stmts);
    bindings.extend(var_names(stmts));
    let visible = parent.child(bindings);
    fold_stmts(stmts, changed, &scope, &visible);
}

fn walk_class(class: &mut ast::Class, changed: &mut bool, visible: &Visible<'_>) {
    for member in &mut class.body {
        match member {
            ast::ClassMember::Method(m) => {
                let params = param_names(m.function.params.iter().map(|p| &p.pat));
                if let Some(body) = &mut m.function.body {
                    fold_body(&mut body.stmts, changed, params, visible);
                }
            }
            ast::ClassMember::PrivateMethod(m) => {
                let params = param_names(m.function.params.iter().map(|p| &p.pat));
                if let Some(body) = &mut m.function.body {
                    fold_body(&mut body.stmts, changed, params, visible);
                }
            }
            ast::ClassMember::Constructor(c) => {
                let mut params = Vec::new();
                for param in &c.params {
                    match param {
                        ast::ParamOrTsParamProp::Param(p) => collect_pat_names(&p.pat, &mut params),
                        ast::ParamOrTsParamProp::TsParamProp(p) => match &p.param {
                            ast::TsParamPropParam::Ident(bi) => params.push(bi.id.sym.to_string()),
                            ast::TsParamPropParam::Assign(a) => {
                                collect_pat_names(&a.left, &mut params)
                            }
                        },
                    }
                }
                if let Some(body) = &mut c.body {
                    fold_body(&mut body.stmts, changed, params, visible);
                }
            }
            ast::ClassMember::StaticBlock(b) => {
                fold_body(&mut b.body.stmts, changed, Vec::new(), visible)
            }
            ast::ClassMember::ClassProp(prop) => {
                if let Some(v) = &mut prop.value {
                    walk_expr(v, changed, visible);
                }
            }
            ast::ClassMember::PrivateProp(prop) => {
                if let Some(v) = &mut prop.value {
                    walk_expr(v, changed, visible);
                }
            }
            _ => {}
        }
    }
}

fn walk_stmt(s: &mut ast::Stmt, changed: &mut bool, scope: &FoldScope, visible: &Visible<'_>) {
    match s {
        ast::Stmt::Block(b) => fold_stmts(&mut b.stmts, changed, scope, visible),
        ast::Stmt::If(i) => {
            walk_stmt(&mut i.cons, changed, scope, visible);
            if let Some(alt) = &mut i.alt {
                walk_stmt(alt, changed, scope, visible);
            }
            walk_expr(&mut i.test, changed, visible);
        }
        ast::Stmt::While(w) => {
            walk_expr(&mut w.test, changed, visible);
            walk_stmt(&mut w.body, changed, scope, visible);
        }
        ast::Stmt::DoWhile(d) => {
            walk_stmt(&mut d.body, changed, scope, visible);
            walk_expr(&mut d.test, changed, visible);
        }
        ast::Stmt::For(f) => {
            let mut head = Vec::new();
            if let Some(ast::VarDeclOrExpr::VarDecl(v)) = &f.init {
                collect_var_decl_names(v, &mut head);
            }
            let visible = &visible.child(head);
            if let Some(ast::VarDeclOrExpr::Expr(e)) = &mut f.init {
                walk_expr(e, changed, visible);
            }
            if let Some(t) = &mut f.test {
                walk_expr(t, changed, visible);
            }
            if let Some(u) = &mut f.update {
                walk_expr(u, changed, visible);
            }
            walk_stmt(&mut f.body, changed, scope, visible);
        }
        ast::Stmt::ForIn(f) => {
            let visible = &visible.child(for_head_names(&f.left));
            walk_stmt(&mut f.body, changed, scope, visible);
        }
        ast::Stmt::ForOf(f) => {
            let visible = &visible.child(for_head_names(&f.left));
            walk_stmt(&mut f.body, changed, scope, visible);
        }
        ast::Stmt::Labeled(l) => walk_stmt(&mut l.body, changed, scope, visible),
        ast::Stmt::Try(t) => {
            fold_stmts(&mut t.block.stmts, changed, scope, visible);
            if let Some(h) = &mut t.handler {
                let mut param = Vec::new();
                if let Some(p) = &h.param {
                    collect_pat_names(p, &mut param);
                }
                fold_stmts(&mut h.body.stmts, changed, scope, &visible.child(param));
            }
            if let Some(f) = &mut t.finalizer {
                fold_stmts(&mut f.stmts, changed, scope, visible);
            }
        }
        ast::Stmt::Switch(sw) => {
            walk_expr(&mut sw.discriminant, changed, visible);
            // A declaration in any case is scoped to the whole switch block.
            let mut declared = Vec::new();
            for case in &sw.cases {
                declared.extend(declared_names(&case.cons));
            }
            let visible = &visible.child(declared);
            for case in &mut sw.cases {
                fold_stmts(&mut case.cons, changed, scope, visible);
            }
        }
        ast::Stmt::Decl(d) => walk_decl(d, changed, visible),
        ast::Stmt::Expr(es) => walk_expr(&mut es.expr, changed, visible),
        ast::Stmt::Return(r) => {
            if let Some(e) = &mut r.arg {
                walk_expr(e, changed, visible);
            }
        }
        ast::Stmt::Throw(t) => walk_expr(&mut t.arg, changed, visible),
        _ => {}
    }
}

/// Recurse into expressions only far enough to find nested function bodies.
fn walk_expr(e: &mut ast::Expr, changed: &mut bool, visible: &Visible<'_>) {
    use ast::Expr as E;
    match e {
        E::Fn(f) => {
            let mut bindings = param_names(f.function.params.iter().map(|p| &p.pat));
            if let Some(ident) = &f.ident {
                bindings.push(ident.sym.to_string());
            }
            if let Some(body) = &mut f.function.body {
                fold_body(&mut body.stmts, changed, bindings, visible);
            }
        }
        E::Arrow(a) => {
            let params = param_names(a.params.iter());
            match &mut *a.body {
                ast::BlockStmtOrExpr::BlockStmt(b) => {
                    fold_body(&mut b.stmts, changed, params, visible)
                }
                ast::BlockStmtOrExpr::Expr(e) => walk_expr(e, changed, &visible.child(params)),
            }
        }
        E::Class(c) => walk_class(&mut c.class, changed, visible),
        E::Array(a) => {
            for el in a.elems.iter_mut().flatten() {
                walk_expr(&mut el.expr, changed, visible);
            }
        }
        E::Object(o) => {
            for p in &mut o.props {
                match p {
                    ast::PropOrSpread::Spread(sp) => walk_expr(&mut sp.expr, changed, visible),
                    ast::PropOrSpread::Prop(prop) => match &mut **prop {
                        ast::Prop::KeyValue(kv) => walk_expr(&mut kv.value, changed, visible),
                        ast::Prop::Method(m) => {
                            let params = param_names(m.function.params.iter().map(|p| &p.pat));
                            if let Some(body) = &mut m.function.body {
                                fold_body(&mut body.stmts, changed, params, visible);
                            }
                        }
                        ast::Prop::Getter(g) => {
                            if let Some(body) = &mut g.body {
                                fold_body(&mut body.stmts, changed, Vec::new(), visible);
                            }
                        }
                        ast::Prop::Setter(sst) => {
                            let params = param_names(std::iter::once(&*sst.param));
                            if let Some(body) = &mut sst.body {
                                fold_body(&mut body.stmts, changed, params, visible);
                            }
                        }
                        _ => {}
                    },
                }
            }
        }
        E::Unary(u) => walk_expr(&mut u.arg, changed, visible),
        E::Update(u) => walk_expr(&mut u.arg, changed, visible),
        E::Bin(b) => {
            walk_expr(&mut b.left, changed, visible);
            walk_expr(&mut b.right, changed, visible);
        }
        E::Assign(a) => walk_expr(&mut a.right, changed, visible),
        E::Member(m) => walk_expr(&mut m.obj, changed, visible),
        E::Cond(c) => {
            walk_expr(&mut c.test, changed, visible);
            walk_expr(&mut c.cons, changed, visible);
            walk_expr(&mut c.alt, changed, visible);
        }
        E::Call(c) => {
            if let ast::Callee::Expr(e) = &mut c.callee {
                walk_expr(e, changed, visible);
            }
            for a in &mut c.args {
                walk_expr(&mut a.expr, changed, visible);
            }
        }
        E::New(n) => {
            walk_expr(&mut n.callee, changed, visible);
            if let Some(args) = &mut n.args {
                for a in args {
                    walk_expr(&mut a.expr, changed, visible);
                }
            }
        }
        E::Seq(s) => {
            for e in &mut s.exprs {
                walk_expr(e, changed, visible);
            }
        }
        E::Tpl(t) => {
            for e in &mut t.exprs {
                walk_expr(e, changed, visible);
            }
        }
        E::Paren(p) => walk_expr(&mut p.expr, changed, visible),
        E::Await(a) => walk_expr(&mut a.arg, changed, visible),
        E::Yield(y) => {
            if let Some(a) = &mut y.arg {
                walk_expr(a, changed, visible);
            }
        }
        E::TsAs(t) => walk_expr(&mut t.expr, changed, visible),
        E::TsNonNull(t) => walk_expr(&mut t.expr, changed, visible),
        E::TsSatisfies(t) => walk_expr(&mut t.expr, changed, visible),
        _ => {}
    }
}

/// Names that resolve to a declarative binding at a point in the source,
/// innermost scope first (#10357). A read of any other name may resolve to a
/// property of the global object, and that property may be an accessor.
///
/// Built only from declarations that cover the WHOLE region they are attached
/// to: a statement list's own declarations, a function's parameters and
/// function-scoped `var`s, a loop head, a catch parameter, and the module's
/// imports and declarations. A declaration in a sibling block is not in the
/// chain, so a same-named read outside it correctly stays unresolved. An
/// ambient `declare` binds nothing at run time and is never collected. A
/// `with` statement anywhere in the module makes every read unresolvable —
/// its object is consulted, through `has`/`get`, before any binding.
struct Visible<'p> {
    names: Vec<String>,
    parent: Option<&'p Visible<'p>>,
    /// Nothing resolves (`with` in the module).
    opaque: bool,
}

impl Visible<'static> {
    /// Resolves only the global constants; for predicates that are never
    /// consulted on an observable builder.
    const ROOT: Visible<'static> = Visible {
        names: Vec::new(),
        parent: None,
        opaque: false,
    };

    fn of_module(module: &ast::Module) -> Self {
        let mut names = Vec::new();
        let mut opaque = false;
        for item in &module.body {
            match item {
                ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import)) => {
                    for specifier in &import.specifiers {
                        let local = match specifier {
                            ast::ImportSpecifier::Named(s) => &s.local,
                            ast::ImportSpecifier::Default(s) => &s.local,
                            ast::ImportSpecifier::Namespace(s) => &s.local,
                        };
                        names.push(local.sym.to_string());
                    }
                }
                ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(e)) => {
                    collect_decl_names(&e.decl, &mut names);
                }
                ast::ModuleItem::Stmt(stmt) => collect_stmt_decl_names(stmt, &mut names),
                _ => {}
            }
        }
        let stmts: Vec<&ast::Stmt> = module
            .body
            .iter()
            .filter_map(|item| match item {
                ast::ModuleItem::Stmt(s) => Some(s),
                _ => None,
            })
            .collect();
        for stmt in stmts {
            let mut vars = VarNames::default();
            stmt.visit_with(&mut vars);
            names.extend(vars.names);
        }
        struct WithFinder(bool);
        impl Visit for WithFinder {
            fn visit_with_stmt(&mut self, _: &ast::WithStmt) {
                self.0 = true;
            }
        }
        let mut with = WithFinder(false);
        module.visit_with(&mut with);
        opaque |= with.0;
        Visible {
            names,
            parent: None,
            opaque,
        }
    }
}

impl<'p> Visible<'p> {
    fn child<'c>(&'c self, names: Vec<String>) -> Visible<'c> {
        Visible {
            names,
            parent: Some(self),
            opaque: self.opaque,
        }
    }

    /// Does a read of `sym` here resolve to a declarative binding, or to a
    /// global constant that cannot be an accessor?
    fn resolves(&self, sym: &str) -> bool {
        if self.opaque {
            return false;
        }
        // Non-writable, non-configurable data properties of the global
        // object: they can never become accessors.
        if matches!(sym, "undefined" | "NaN" | "Infinity") {
            return true;
        }
        let mut scope = Some(self);
        while let Some(visible) = scope {
            if visible.names.iter().any(|n| n == sym) {
                return true;
            }
            scope = visible.parent;
        }
        false
    }
}

/// The declarations made directly by a statement list.
fn declared_names(stmts: &[ast::Stmt]) -> Vec<String> {
    let mut names = Vec::new();
    for stmt in stmts {
        collect_stmt_decl_names(stmt, &mut names);
    }
    names
}

fn collect_stmt_decl_names(stmt: &ast::Stmt, names: &mut Vec<String>) {
    if let ast::Stmt::Decl(decl) = stmt {
        collect_decl_names(decl, names);
    }
}

fn collect_decl_names(decl: &ast::Decl, names: &mut Vec<String>) {
    match decl {
        ast::Decl::Var(v) => collect_var_decl_names(v, names),
        ast::Decl::Fn(f) if !f.declare => names.push(f.ident.sym.to_string()),
        ast::Decl::Class(c) if !c.declare => names.push(c.ident.sym.to_string()),
        ast::Decl::TsEnum(e) if !e.declare => names.push(e.id.sym.to_string()),
        _ => {}
    }
}

fn collect_var_decl_names(v: &ast::VarDecl, names: &mut Vec<String>) {
    if v.declare {
        return;
    }
    for d in &v.decls {
        collect_pat_names(&d.name, names);
    }
}

fn for_head_names(head: &ast::ForHead) -> Vec<String> {
    let mut names = Vec::new();
    if let ast::ForHead::VarDecl(v) = head {
        collect_var_decl_names(v, &mut names);
    }
    names
}

fn param_names<'a>(pats: impl Iterator<Item = &'a ast::Pat>) -> Vec<String> {
    let mut names = Vec::new();
    for pat in pats {
        collect_pat_names(pat, &mut names);
    }
    names
}

/// Binding names of a pattern. Default values are not walked: a closure in a
/// default binds nothing in this scope.
fn collect_pat_names(pat: &ast::Pat, names: &mut Vec<String>) {
    match pat {
        ast::Pat::Ident(bi) => names.push(bi.id.sym.to_string()),
        ast::Pat::Array(a) => {
            for elem in a.elems.iter().flatten() {
                collect_pat_names(elem, names);
            }
        }
        ast::Pat::Rest(r) => collect_pat_names(&r.arg, names),
        ast::Pat::Object(o) => {
            for prop in &o.props {
                match prop {
                    ast::ObjectPatProp::KeyValue(kv) => collect_pat_names(&kv.value, names),
                    ast::ObjectPatProp::Assign(a) => names.push(a.key.id.sym.to_string()),
                    ast::ObjectPatProp::Rest(r) => collect_pat_names(&r.arg, names),
                }
            }
        }
        ast::Pat::Assign(a) => collect_pat_names(&a.left, names),
        ast::Pat::Invalid(_) | ast::Pat::Expr(_) => {}
    }
}

/// Function-scoped `var` names anywhere in a body, not inside a nested
/// function-like.
fn var_names(stmts: &[ast::Stmt]) -> Vec<String> {
    let mut vars = VarNames::default();
    stmts.visit_with(&mut vars);
    vars.names
}

#[derive(Default)]
struct VarNames {
    names: Vec<String>,
}

impl Visit for VarNames {
    fn visit_var_decl(&mut self, v: &ast::VarDecl) {
        if v.kind == ast::VarDeclKind::Var {
            collect_var_decl_names(v, &mut self.names);
        }
        v.visit_children_with(self);
    }
    fn visit_function(&mut self, _: &ast::Function) {}
    fn visit_arrow_expr(&mut self, _: &ast::ArrowExpr) {}
    fn visit_constructor(&mut self, _: &ast::Constructor) {}
    fn visit_getter_prop(&mut self, _: &ast::GetterProp) {}
    fn visit_setter_prop(&mut self, _: &ast::SetterProp) {}
    fn visit_class_prop(&mut self, _: &ast::ClassProp) {}
    fn visit_private_prop(&mut self, _: &ast::PrivateProp) {}
    fn visit_auto_accessor(&mut self, _: &ast::AutoAccessor) {}
    fn visit_static_block(&mut self, _: &ast::StaticBlock) {}
}

// #6812 (w16): compile-time builder WIDTH scan, split into
// `builder_fold_width_hints.rs` to keep this file under the 2000-line cap.
#[path = "builder_fold_width_hints.rs"]
mod width_hints;
pub(crate) use width_hints::empty_builder_width_hints;
