//! Handle-free `createHash` / `createHmac` for hash objects that never escape
//! (#11516).
//!
//! #11515 made hash/HMAC handles reclaimable: an id is reused only after a
//! full trace proves no JS value names it. That fixed id exhaustion but put
//! full-trace work on every request that hashes. The dominant per-request
//! shapes never let the hash object escape, so this pass proves that and
//! rewrites them onto `perry_hir::crypto_chain`'s internal calls, which
//! codegen lowers to a digest state in a stack slot of the current frame: no
//! handle is registered, so there is nothing to park, trace or reclaim.
//!
//! Two shapes are recognised:
//!
//! * **Chains** — `crypto.createHash(a[, opts]).update(x[, enc])*.digest([e])`
//!   and the `createHmac(a, key)` / legacy `Hash` / `Hmac` forms, with any
//!   number of `update` calls. The object is a temporary no expression can
//!   name, so it cannot escape.
//! * **Block-local objects** — `const h = crypto.createHash(a)` (optionally
//!   followed by `.update(..)` calls in the initializer) whose every other
//!   reference, module-wide, is the receiver of an `h.update(..)` whose
//!   result is discarded or chained into another `update`/`digest`, or of an
//!   `h.digest(..)`. Those references must follow the declaration in the
//!   same statement list and sit outside any closure, and no `await`/`yield`
//!   may run between the declaration and the last reference (a suspension
//!   would leave the frame that owns the slot).
//!
//! Anything else — the object stored, passed, returned, captured, compared,
//! `copy()`d, used as a stream (`write`/`end`/`on`/`pipe`), read as a
//! property, or reached through a spread/optional call — keeps the
//! registered-handle path. The occurrence count is taken over every LocalId
//! carrier in the module (the same containers as the hardened
//! `generator::compute_max_local_id` scan), so a reference this pass does not
//! understand is a mismatch, never a missed escape.
//!
//! Semantics are unchanged: the runtime entry points reuse the handle path's
//! state constructors, update decoding and digest encoding, and a finalized
//! state still throws `ERR_CRYPTO_HASH_FINALIZED` on `update`/second `digest`.
//! Each init site owns one slot per frame; re-running the declaration can
//! only happen on a fresh entry into its block, after every reference to the
//! previous object is dead (none survive in closures, by the count above).
//!
//! Runs after the inliner (so inlined helper bodies are seen) and before the
//! async/generator transforms (which box locals into cells and split
//! expressions at `await`).

use std::collections::HashMap;

use perry_hir::crypto_chain::{CHAIN_DIGEST, CHAIN_INIT_HASH, CHAIN_INIT_HMAC, CHAIN_UPDATE};
use perry_hir::types::{LocalId, Type};
use perry_hir::walker::{walk_expr_children, walk_expr_children_mut};
use perry_hir::{Decorator, Expr, Function, Module, Param, Stmt};

pub fn run(module: &mut Module) {
    if !module_has_create_call(module) {
        return;
    }
    let counts = count_module_occurrences(module);
    let mut rw = Rewriter {
        counts: &counts,
        require_const: true,
    };
    // Module scope: a top-level binding can be exported or become a script
    // global, both of which name it without a LocalId, so only nested blocks
    // are considered, and only `const` there (a nested `var` would still be a
    // script global). Closure bodies reset this.
    for stmt in &mut module.init {
        rw.stmt(stmt);
    }
    rw.require_const = false;
    for f in &mut module.functions {
        rw.function(f);
    }
    for class in &mut module.classes {
        for f in class
            .constructor
            .iter_mut()
            .chain(class.methods.iter_mut())
            .chain(class.static_methods.iter_mut())
            .chain(class.getters.iter_mut().map(|(_, f)| f))
            .chain(class.setters.iter_mut().map(|(_, f)| f))
            .chain(class.computed_members.iter_mut().map(|m| &mut m.function))
        {
            rw.function(f);
        }
    }
}

// ---------------------------------------------------------------------------
// Shape recognition
// ---------------------------------------------------------------------------

fn is_crypto(expr: &Expr) -> bool {
    matches!(expr, Expr::NativeModuleRef(n) if n == "crypto")
}

/// `crypto.createHash(alg, ..)` / `crypto.Hash(alg, ..)` → `Some(false)`;
/// `crypto.createHmac(alg, key, ..)` / `crypto.Hmac(..)` → `Some(true)`.
/// A call without the required arguments stays on the handle path, which
/// owns its (non-throwing) degenerate behaviour.
fn create_call_kind(expr: &Expr) -> Option<bool> {
    let Expr::Call { callee, args, .. } = expr else {
        return None;
    };
    let Expr::PropertyGet {
        object, property, ..
    } = callee.as_ref()
    else {
        return None;
    };
    if !is_crypto(object) {
        return None;
    }
    match property.as_str() {
        "createHash" | "Hash" if !args.is_empty() => Some(false),
        "createHmac" | "Hmac" if args.len() >= 2 => Some(true),
        _ => None,
    }
}

/// `recv.<method>(args)` with a plain `Expr::Call` (spread and optional
/// calls are other variants and are never matched).
fn method_call<'a>(expr: &'a Expr, method: &str) -> Option<(&'a Expr, &'a [Expr])> {
    let Expr::Call { callee, args, .. } = expr else {
        return None;
    };
    match callee.as_ref() {
        Expr::PropertyGet {
            object, property, ..
        } if property == method => Some((object.as_ref(), args.as_slice())),
        _ => None,
    }
}

/// Peel `.update(..)` calls off `expr`; returns the innermost receiver.
fn peel_updates(mut expr: &Expr) -> &Expr {
    while let Some((recv, _)) = method_call(expr, "update") {
        expr = recv;
    }
    expr
}

/// `createX(..).update(..)*.digest(..)` — an inline chain whose object no
/// expression can name.
fn is_inline_chain(expr: &Expr) -> bool {
    method_call(expr, "digest")
        .is_some_and(|(recv, _)| create_call_kind(peel_updates(recv)).is_some())
}

/// The chains codegen already collapses to one direct helper with no handle
/// (`js_crypto_sha256` / `js_crypto_hmac_sha256` on a literal string).
/// Leave them to that arm.
fn is_literal_fast_path(expr: &Expr) -> bool {
    let Some((recv, digest_args)) = method_call(expr, "digest") else {
        return false;
    };
    let Some((create, update_args)) = method_call(recv, "update") else {
        return false;
    };
    let Expr::Call {
        callee,
        args: create_args,
        ..
    } = create
    else {
        return false;
    };
    let Expr::PropertyGet { property, .. } = callee.as_ref() else {
        return false;
    };
    let enc_ok = match digest_args.first() {
        None | Some(Expr::Undefined) => true,
        Some(Expr::String(s)) => s.eq_ignore_ascii_case("hex"),
        _ => false,
    };
    let data_ok = update_args.len() == 1 && matches!(update_args[0], Expr::String(_));
    let sha256 = matches!(create_args.first(), Some(Expr::String(a)) if a == "sha256");
    enc_ok
        && data_ok
        && sha256
        && match property.as_str() {
            "createHash" => true,
            "createHmac" => matches!(create_args.get(1), Some(Expr::String(_))),
            _ => false,
        }
}

fn contains_suspension(expr: &Expr) -> bool {
    match expr {
        Expr::Await(_) | Expr::Yield { .. } => return true,
        // A closure body runs in its own frame, later.
        Expr::Closure { .. } => return false,
        _ => {}
    }
    let mut found = false;
    walk_expr_children(expr, &mut |child| {
        if !found && contains_suspension(child) {
            found = true;
        }
    });
    found
}

fn stmt_contains_suspension(stmt: &Stmt) -> bool {
    let mut found = false;
    for_each_stmt_expr(stmt, &mut |e| {
        if !found && contains_suspension(e) {
            found = true;
        }
    });
    found
}

/// Visit every expression a statement evaluates in its own frame
/// (sub-statements included, closure bodies excluded).
fn for_each_stmt_expr(stmt: &Stmt, f: &mut dyn FnMut(&Expr)) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                f(e);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => f(e),
        Stmt::Return(e) => {
            if let Some(e) = e {
                f(e);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            f(condition);
            then_branch.iter().for_each(|s| for_each_stmt_expr(s, f));
            if let Some(b) = else_branch {
                b.iter().for_each(|s| for_each_stmt_expr(s, f));
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            f(condition);
            body.iter().for_each(|s| for_each_stmt_expr(s, f));
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                for_each_stmt_expr(i, f);
            }
            if let Some(c) = condition {
                f(c);
            }
            if let Some(u) = update {
                f(u);
            }
            body.iter().for_each(|s| for_each_stmt_expr(s, f));
        }
        Stmt::Labeled { body, .. } => for_each_stmt_expr(body, f),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().for_each(|s| for_each_stmt_expr(s, f));
            if let Some(c) = catch {
                c.body.iter().for_each(|s| for_each_stmt_expr(s, f));
            }
            if let Some(fin) = finally {
                fin.iter().for_each(|s| for_each_stmt_expr(s, f));
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            f(discriminant);
            for case in cases {
                if let Some(t) = &case.test {
                    f(t);
                }
                case.body.iter().for_each(|s| for_each_stmt_expr(s, f));
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}

// ---------------------------------------------------------------------------
// Module-wide occurrence count of every LocalId carrier
// ---------------------------------------------------------------------------

fn module_has_create_call(module: &Module) -> bool {
    fn expr_has(e: &Expr) -> bool {
        if create_call_kind(e).is_some() {
            return true;
        }
        if let Expr::Closure { body, .. } = e {
            if body.iter().any(stmt_has) {
                return true;
            }
        }
        let mut found = false;
        walk_expr_children(e, &mut |c| {
            if !found && expr_has(c) {
                found = true;
            }
        });
        found
    }
    fn stmt_has(s: &Stmt) -> bool {
        let mut found = false;
        for_each_stmt_expr(s, &mut |e| {
            if !found && expr_has(e) {
                found = true;
            }
        });
        found
    }
    module.init.iter().any(stmt_has)
        || module.functions.iter().any(|f| f.body.iter().any(stmt_has))
        || module.classes.iter().any(|c| {
            c.constructor
                .iter()
                .chain(&c.methods)
                .chain(&c.static_methods)
                .chain(c.getters.iter().map(|(_, f)| f))
                .chain(c.setters.iter().map(|(_, f)| f))
                .chain(c.computed_members.iter().map(|m| &m.function))
                .any(|f| f.body.iter().any(stmt_has))
        })
}

type Counts = HashMap<LocalId, usize>;

fn bump(counts: &mut Counts, id: LocalId) {
    *counts.entry(id).or_insert(0) += 1;
}

/// Count every place a LocalId appears, in every container of the module.
/// Over-counting only makes the pass bail; the container list mirrors
/// `generator::compute_max_local_id` so nothing is under-counted.
fn count_module_occurrences(module: &Module) -> Counts {
    let mut c = Counts::new();
    for f in &module.functions {
        count_function(f, &mut c);
    }
    count_stmts(&module.init, &mut c);
    for g in &module.globals {
        bump(&mut c, g.id);
        if let Some(init) = &g.init {
            count_expr(init, &mut c);
        }
    }
    for class in &module.classes {
        for f in class
            .constructor
            .iter()
            .chain(&class.methods)
            .chain(&class.static_methods)
            .chain(class.getters.iter().map(|(_, f)| f))
            .chain(class.setters.iter().map(|(_, f)| f))
        {
            count_function(f, &mut c);
        }
        for m in &class.computed_members {
            count_expr(&m.key_expr, &mut c);
            count_function(&m.function, &mut c);
        }
        for field in class.fields.iter().chain(&class.static_fields) {
            if let Some(init) = &field.init {
                count_expr(init, &mut c);
            }
            if let Some(key) = &field.key_expr {
                count_expr(key, &mut c);
            }
            count_decorators(&field.decorators, &mut c);
        }
        count_decorators(&class.decorators, &mut c);
        if let Some(e) = &class.extends_expr {
            count_expr(e, &mut c);
        }
    }
    c
}

fn count_decorators(decorators: &[Decorator], c: &mut Counts) {
    for d in decorators {
        for a in &d.args {
            count_expr(a, c);
        }
    }
}

fn count_params(params: &[Param], c: &mut Counts) {
    for p in params {
        bump(c, p.id);
        if let Some(d) = &p.default {
            count_expr(d, c);
        }
        count_decorators(&p.decorators, c);
        if let Some(meta) = &p.arguments_object {
            for (_, local) in &meta.mapped_parameter_ids {
                bump(c, *local);
            }
        }
    }
}

fn count_function(f: &Function, c: &mut Counts) {
    count_params(&f.params, c);
    count_decorators(&f.decorators, c);
    for id in &f.captures {
        bump(c, *id);
    }
    count_stmts(&f.body, c);
}

fn count_stmts(stmts: &[Stmt], c: &mut Counts) {
    for s in stmts {
        count_stmt(s, c);
    }
}

fn count_stmt(stmt: &Stmt, c: &mut Counts) {
    match stmt {
        Stmt::Let { id, init, .. } => {
            bump(c, *id);
            if let Some(e) = init {
                count_expr(e, c);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => count_expr(e, c),
        Stmt::Return(e) => {
            if let Some(e) = e {
                count_expr(e, c);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            count_expr(condition, c);
            count_stmts(then_branch, c);
            if let Some(b) = else_branch {
                count_stmts(b, c);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            count_expr(condition, c);
            count_stmts(body, c);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                count_stmt(i, c);
            }
            if let Some(e) = condition {
                count_expr(e, c);
            }
            if let Some(e) = update {
                count_expr(e, c);
            }
            count_stmts(body, c);
        }
        Stmt::Labeled { body, .. } => count_stmt(body, c),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            count_stmts(body, c);
            if let Some(catch) = catch {
                if let Some((id, _)) = catch.param {
                    bump(c, id);
                }
                count_stmts(&catch.body, c);
            }
            if let Some(f) = finally {
                count_stmts(f, c);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            count_expr(discriminant, c);
            for case in cases {
                if let Some(t) = &case.test {
                    count_expr(t, c);
                }
                count_stmts(&case.body, c);
            }
        }
        Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) | Stmt::ReleaseBoxes(ids) => {
            for id in ids {
                bump(c, *id);
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
    }
}

fn count_expr(expr: &Expr, c: &mut Counts) {
    match expr {
        Expr::LocalGet(id) | Expr::LocalSet(id, _) | Expr::Update { id, .. } => bump(c, *id),
        Expr::ArrayPush { array_id, .. }
        | Expr::ArrayPushSpread { array_id, .. }
        | Expr::ArrayUnshift { array_id, .. }
        | Expr::ArraySplice { array_id, .. }
        | Expr::ArrayCopyWithin { array_id, .. } => bump(c, *array_id),
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => bump(c, *id),
        Expr::SetAdd { set_id, .. } => bump(c, *set_id),
        Expr::Closure {
            params,
            body,
            captures,
            mutable_captures,
            ..
        } => {
            // Param defaults are reached by the walker below.
            for p in params {
                bump(c, p.id);
                count_decorators(&p.decorators, c);
                if let Some(meta) = &p.arguments_object {
                    for (_, local) in &meta.mapped_parameter_ids {
                        bump(c, *local);
                    }
                }
            }
            for id in captures.iter().chain(mutable_captures) {
                bump(c, *id);
            }
            count_stmts(body, c);
        }
        _ => {}
    }
    walk_expr_children(expr, &mut |child| count_expr(child, c));
}

// ---------------------------------------------------------------------------
// Block-local escape analysis
// ---------------------------------------------------------------------------

/// Allowed references to `id` in `expr`, evaluated in the current frame.
/// `discarded`: the expression's value is thrown away.
fn allowed_refs(expr: &Expr, id: LocalId, discarded: bool) -> usize {
    let root_is_id = |recv: &Expr| matches!(peel_updates(recv), Expr::LocalGet(l) if *l == id);
    let chain_args_refs = |mut e: &Expr, n: &mut usize| {
        // Walk the receiver chain down to the root, counting refs in args.
        loop {
            let Expr::Call { callee, args, .. } = e else {
                break;
            };
            for a in args {
                *n += allowed_refs(a, id, false);
            }
            match callee.as_ref() {
                Expr::PropertyGet { object, .. } => e = object,
                _ => break,
            }
        }
    };
    if let Some((recv, _)) = method_call(expr, "digest") {
        if root_is_id(recv) {
            let mut n = 1;
            chain_args_refs(expr, &mut n);
            return n;
        }
    }
    if discarded {
        if let Some((recv, _)) = method_call(expr, "update") {
            if root_is_id(recv) {
                let mut n = 1;
                chain_args_refs(expr, &mut n);
                return n;
            }
        }
    }
    match expr {
        Expr::Closure { .. } => 0,
        Expr::Sequence(items) => {
            let last = items.len().saturating_sub(1);
            items
                .iter()
                .enumerate()
                .map(|(i, e)| allowed_refs(e, id, i < last || discarded))
                .sum()
        }
        _ => {
            let mut n = 0;
            walk_expr_children(expr, &mut |child| n += allowed_refs(child, id, false));
            n
        }
    }
}

fn allowed_refs_stmt(stmt: &Stmt, id: LocalId) -> usize {
    match stmt {
        Stmt::Expr(e) => allowed_refs(e, id, true),
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_deref().map_or(0, |s| allowed_refs_stmt(s, id))
                + condition.as_ref().map_or(0, |e| allowed_refs(e, id, false))
                + update.as_ref().map_or(0, |e| allowed_refs(e, id, true))
                + body.iter().map(|s| allowed_refs_stmt(s, id)).sum::<usize>()
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            allowed_refs(condition, id, false)
                + then_branch
                    .iter()
                    .map(|s| allowed_refs_stmt(s, id))
                    .sum::<usize>()
                + else_branch
                    .iter()
                    .flatten()
                    .map(|s| allowed_refs_stmt(s, id))
                    .sum::<usize>()
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            allowed_refs(condition, id, false)
                + body.iter().map(|s| allowed_refs_stmt(s, id)).sum::<usize>()
        }
        Stmt::Labeled { body, .. } => allowed_refs_stmt(body, id),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().map(|s| allowed_refs_stmt(s, id)).sum::<usize>()
                + catch
                    .iter()
                    .flat_map(|c| &c.body)
                    .map(|s| allowed_refs_stmt(s, id))
                    .sum::<usize>()
                + finally
                    .iter()
                    .flatten()
                    .map(|s| allowed_refs_stmt(s, id))
                    .sum::<usize>()
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            allowed_refs(discriminant, id, false)
                + cases
                    .iter()
                    .map(|case| {
                        case.test.as_ref().map_or(0, |t| allowed_refs(t, id, false))
                            + case
                                .body
                                .iter()
                                .map(|s| allowed_refs_stmt(s, id))
                                .sum::<usize>()
                    })
                    .sum::<usize>()
        }
        Stmt::Let { init, .. } => init.as_ref().map_or(0, |e| allowed_refs(e, id, false)),
        Stmt::Return(e) => e.as_ref().map_or(0, |e| allowed_refs(e, id, false)),
        Stmt::Throw(e) => allowed_refs(e, id, false),
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => 0,
    }
}

// ---------------------------------------------------------------------------
// Rewriting
// ---------------------------------------------------------------------------

fn crypto_call(method: &str, args: Vec<Expr>, byte_offset: u32) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::NativeModuleRef("crypto".to_string())),
            property: method.to_string(),
            byte_offset,
        }),
        args,
        type_args: vec![],
        byte_offset,
    }
}

/// `createX(args)` → `crypto.__perry*ChainInit(args)`.
fn rewrite_create(expr: Expr) -> Expr {
    let is_hmac = create_call_kind(&expr) == Some(true);
    let Expr::Call {
        args, byte_offset, ..
    } = expr
    else {
        unreachable!("checked by create_call_kind")
    };
    let init = if is_hmac {
        CHAIN_INIT_HMAC
    } else {
        CHAIN_INIT_HASH
    };
    crypto_call(init, args, byte_offset)
}

/// Rewrite `recv.update(..)` / `recv.digest(..)` into the chain calls,
/// recursing down the receiver chain. `root` rewrites the chain's root.
fn rewrite_chain(expr: Expr, root: &mut dyn FnMut(Expr) -> Expr) -> Expr {
    let Expr::Call {
        callee,
        args,
        byte_offset,
        ..
    } = expr
    else {
        return root(expr);
    };
    let Expr::PropertyGet {
        object, property, ..
    } = *callee
    else {
        unreachable!("chain links are method calls")
    };
    let method = match property.as_str() {
        "update" => CHAIN_UPDATE,
        "digest" => CHAIN_DIGEST,
        _ => {
            // The root create call itself.
            return root(Expr::Call {
                callee: Box::new(Expr::PropertyGet {
                    object,
                    property,
                    byte_offset,
                }),
                args,
                type_args: vec![],
                byte_offset,
            });
        }
    };
    let state = rewrite_chain(*object, root);
    let mut new_args = Vec::with_capacity(args.len() + 1);
    new_args.push(state);
    new_args.extend(args);
    crypto_call(method, new_args, byte_offset)
}

struct Rewriter<'a> {
    counts: &'a Counts,
    /// Only `const` block-locals qualify (nested blocks of module init).
    require_const: bool,
}

impl Rewriter<'_> {
    fn function(&mut self, f: &mut Function) {
        for s in &mut f.body {
            self.stmt(s);
        }
        self.stmts(&mut f.body);
    }

    /// Recurse into every nested statement list and closure body, and
    /// rewrite inline chains in expressions.
    fn stmt(&mut self, stmt: &mut Stmt) {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    self.expr(e);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => self.expr(e),
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.expr(e);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition);
                self.block(then_branch);
                if let Some(b) = else_branch {
                    self.block(b);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.expr(condition);
                self.block(body);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                if let Some(c) = condition {
                    self.expr(c);
                }
                if let Some(u) = update {
                    self.expr(u);
                }
                self.block(body);
            }
            Stmt::Labeled { body, .. } => self.stmt(body),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.block(body);
                if let Some(c) = catch {
                    self.block(&mut c.body);
                }
                if let Some(f) = finally {
                    self.block(f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant);
                for case in cases {
                    if let Some(t) = &mut case.test {
                        self.expr(t);
                    }
                    self.block(&mut case.body);
                }
            }
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
        }
    }

    fn block(&mut self, stmts: &mut Vec<Stmt>) {
        for s in stmts.iter_mut() {
            self.stmt(s);
        }
        self.stmts(stmts);
    }

    fn expr(&mut self, expr: &mut Expr) {
        if is_inline_chain(expr) && !is_literal_fast_path(expr) && !contains_suspension(expr) {
            let taken = std::mem::replace(expr, Expr::Undefined);
            let mut rewritten = rewrite_chain(taken, &mut rewrite_create);
            // Chains nested in the arguments (`update(createHash(..)...)`).
            walk_expr_children_mut(&mut rewritten, &mut |c| self.expr(c));
            *expr = rewritten;
            return;
        }
        if let Expr::Closure { body, .. } = expr {
            let outer = std::mem::replace(&mut self.require_const, false);
            self.block(body);
            self.require_const = outer;
        }
        walk_expr_children_mut(expr, &mut |c| self.expr(c));
    }

    /// Block-local objects declared directly in `stmts`.
    fn stmts(&mut self, stmts: &mut [Stmt]) {
        for i in 0..stmts.len() {
            let Stmt::Let {
                id,
                init: Some(init),
                mutable,
                ..
            } = &stmts[i]
            else {
                continue;
            };
            if *mutable && self.require_const {
                continue;
            }
            let id = *id;
            if create_call_kind(peel_updates(init)).is_none() || contains_suspension(init) {
                continue;
            }
            let mut allowed = 0;
            let mut last = i;
            for (j, s) in stmts.iter().enumerate().skip(i + 1) {
                let n = allowed_refs_stmt(s, id);
                if n > 0 {
                    allowed += n;
                    last = j;
                }
            }
            if allowed == 0 || self.counts.get(&id).copied() != Some(1 + allowed) {
                continue;
            }
            if stmts[i + 1..=last].iter().any(stmt_contains_suspension) {
                continue;
            }
            // Proven: rewrite the declaration and every reference.
            if let Stmt::Let { init, ty, .. } = &mut stmts[i] {
                let taken = init.take().expect("matched above");
                *init = Some(rewrite_chain(taken, &mut rewrite_create));
                // The binding now holds an opaque frame address, not a Hash.
                *ty = Type::Any;
            }
            for s in &mut stmts[i + 1..=last] {
                rewrite_refs_stmt(s, id);
            }
        }
    }
}

fn rewrite_refs_expr(expr: &mut Expr, id: LocalId) {
    let is_ref_chain = |e: &Expr| {
        ["update", "digest"].iter().any(|m| {
            method_call(e, m).is_some_and(
                |(recv, _)| matches!(peel_updates(recv), Expr::LocalGet(l) if *l == id),
            )
        })
    };
    if is_ref_chain(expr) {
        let taken = std::mem::replace(expr, Expr::Undefined);
        let mut rewritten = rewrite_chain(taken, &mut |root| root);
        // Refs nested in this chain's arguments (`h.update(h.digest())` is
        // not allowed, but a nested *other* object's chain is untouched).
        rewrite_chain_args(&mut rewritten, id);
        *expr = rewritten;
        return;
    }
    if matches!(expr, Expr::Closure { .. }) {
        return;
    }
    walk_expr_children_mut(expr, &mut |c| rewrite_refs_expr(c, id));
}

/// Recurse into the non-state arguments of a rewritten chain.
fn rewrite_chain_args(expr: &mut Expr, id: LocalId) {
    let Expr::Call { args, .. } = expr else {
        return;
    };
    let mut iter = args.iter_mut();
    if let Some(state) = iter.next() {
        rewrite_chain_args(state, id);
    }
    for a in iter {
        rewrite_refs_expr(a, id);
    }
}

fn rewrite_refs_stmt(stmt: &mut Stmt, id: LocalId) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                rewrite_refs_expr(e, id);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => rewrite_refs_expr(e, id),
        Stmt::Return(e) => {
            if let Some(e) = e {
                rewrite_refs_expr(e, id);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_refs_expr(condition, id);
            then_branch
                .iter_mut()
                .for_each(|s| rewrite_refs_stmt(s, id));
            if let Some(b) = else_branch {
                b.iter_mut().for_each(|s| rewrite_refs_stmt(s, id));
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            rewrite_refs_expr(condition, id);
            body.iter_mut().for_each(|s| rewrite_refs_stmt(s, id));
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                rewrite_refs_stmt(i, id);
            }
            if let Some(c) = condition {
                rewrite_refs_expr(c, id);
            }
            if let Some(u) = update {
                rewrite_refs_expr(u, id);
            }
            body.iter_mut().for_each(|s| rewrite_refs_stmt(s, id));
        }
        Stmt::Labeled { body, .. } => rewrite_refs_stmt(body, id),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter_mut().for_each(|s| rewrite_refs_stmt(s, id));
            if let Some(c) = catch {
                c.body.iter_mut().for_each(|s| rewrite_refs_stmt(s, id));
            }
            if let Some(f) = finally {
                f.iter_mut().for_each(|s| rewrite_refs_stmt(s, id));
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            rewrite_refs_expr(discriminant, id);
            for case in cases {
                if let Some(t) = &mut case.test {
                    rewrite_refs_expr(t, id);
                }
                case.body.iter_mut().for_each(|s| rewrite_refs_stmt(s, id));
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}

#[cfg(test)]
#[path = "crypto_hash_chain_tests.rs"]
mod tests;
