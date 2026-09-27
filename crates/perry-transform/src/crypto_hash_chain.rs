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

use std::collections::{HashMap, HashSet};

use perry_hir::crypto_chain::{CHAIN_DIGEST, CHAIN_INIT_HASH, CHAIN_INIT_HMAC, CHAIN_UPDATE};
use perry_hir::types::{LocalId, Type};
use perry_hir::walker::{walk_expr_children, walk_expr_children_mut};
use perry_hir::{Decorator, Expr, Function, Module, Param, Stmt};

pub fn run(module: &mut Module) {
    let aliases = cjs_crypto_aliases(module);
    if !module_has_create_call(module, &aliases) {
        return;
    }
    let counts = count_module_occurrences(module);
    let placeholders = count_placeholder_lets(module);
    let mut rw = Rewriter {
        counts: &counts,
        aliases: &aliases,
        placeholders: &placeholders,
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

/// The `node:crypto` namespace: the import binding, or a CommonJS
/// `var crypto = require('crypto')` alias (see [`cjs_crypto_aliases`]).
fn is_crypto(expr: &Expr, aliases: &HashSet<LocalId>) -> bool {
    match expr {
        Expr::NativeModuleRef(n) => n == "crypto",
        Expr::LocalGet(id) => aliases.contains(id),
        _ => false,
    }
}

/// `crypto.createHash(alg, ..)` / `crypto.Hash(alg, ..)` → `Some(false)`;
/// `crypto.createHmac(alg, key, ..)` / `crypto.Hmac(..)` → `Some(true)`.
/// A call without the required arguments stays on the handle path, which
/// owns its (non-throwing) degenerate behaviour.
fn create_call_kind(expr: &Expr, aliases: &HashSet<LocalId>) -> Option<bool> {
    let Expr::Call { callee, args, .. } = expr else {
        return None;
    };
    let Expr::PropertyGet {
        object, property, ..
    } = callee.as_ref()
    else {
        return None;
    };
    if !is_crypto(object, aliases) {
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
fn is_inline_chain(expr: &Expr, aliases: &HashSet<LocalId>) -> bool {
    method_call(expr, "digest")
        .is_some_and(|(recv, _)| create_call_kind(peel_updates(recv), aliases).is_some())
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
    let Expr::PropertyGet {
        property, object, ..
    } = callee.as_ref()
    else {
        return false;
    };
    // Codegen's direct helpers only match the import binding.
    if !matches!(object.as_ref(), Expr::NativeModuleRef(n) if n == "crypto") {
        return false;
    }
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

fn module_has_create_call(module: &Module, aliases: &HashSet<LocalId>) -> bool {
    fn expr_has(e: &Expr, aliases: &HashSet<LocalId>) -> bool {
        if create_call_kind(e, aliases).is_some() {
            return true;
        }
        if let Expr::Closure { body, .. } = e {
            if body.iter().any(|s| stmt_has(s, aliases)) {
                return true;
            }
        }
        let mut found = false;
        walk_expr_children(e, &mut |c| {
            if !found && expr_has(c, aliases) {
                found = true;
            }
        });
        found
    }
    fn stmt_has(s: &Stmt, aliases: &HashSet<LocalId>) -> bool {
        let mut found = false;
        for_each_stmt_expr(s, &mut |e| {
            if !found && expr_has(e, aliases) {
                found = true;
            }
        });
        found
    }
    let stmt_has = |s: &Stmt| stmt_has(s, aliases);
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

/// Visit every statement list in the module's function bodies, init and
/// closure bodies (the places a `Let` can appear).
fn for_each_let(module: &Module, f: &mut dyn FnMut(&Stmt, &[Stmt])) {
    fn stmts(list: &[Stmt], f: &mut dyn FnMut(&Stmt, &[Stmt])) {
        for s in list {
            stmt(s, list, f);
        }
    }
    fn stmt(s: &Stmt, parent: &[Stmt], f: &mut dyn FnMut(&Stmt, &[Stmt])) {
        f(s, parent);
        match s {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                stmts(then_branch, f);
                if let Some(b) = else_branch {
                    stmts(b, f);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => stmts(body, f),
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    stmt(i, parent, f);
                }
                stmts(body, f);
            }
            Stmt::Labeled { body, .. } => stmt(body, parent, f),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                stmts(body, f);
                if let Some(c) = catch {
                    stmts(&c.body, f);
                }
                if let Some(fin) = finally {
                    stmts(fin, f);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    stmts(&c.body, f);
                }
            }
            _ => {}
        }
        for_each_stmt_expr_shallow(s, &mut |e| closures_in(e, f));
    }
    fn closures_in(e: &Expr, f: &mut dyn FnMut(&Stmt, &[Stmt])) {
        if let Expr::Closure { body, .. } = e {
            stmts(body, f);
        }
        walk_expr_children(e, &mut |c| closures_in(c, f));
    }
    stmts(&module.init, f);
    for func in &module.functions {
        stmts(&func.body, f);
    }
    for class in &module.classes {
        for func in class
            .constructor
            .iter()
            .chain(&class.methods)
            .chain(&class.static_methods)
            .chain(class.getters.iter().map(|(_, f)| f))
            .chain(class.setters.iter().map(|(_, f)| f))
            .chain(class.computed_members.iter().map(|m| &m.function))
        {
            stmts(&func.body, f);
        }
    }
}

/// The expressions a statement evaluates itself (not its sub-statements).
fn for_each_stmt_expr_shallow(stmt: &Stmt, f: &mut dyn FnMut(&Expr)) {
    match stmt {
        Stmt::Let { init: Some(e), .. } | Stmt::Expr(e) | Stmt::Throw(e) => f(e),
        Stmt::Return(Some(e)) => f(e),
        Stmt::If { condition, .. }
        | Stmt::While { condition, .. }
        | Stmt::DoWhile { condition, .. } => f(condition),
        Stmt::For {
            condition, update, ..
        } => {
            if let Some(c) = condition {
                f(c);
            }
            if let Some(u) = update {
                f(u);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            f(discriminant);
            for c in cases {
                if let Some(t) = &c.test {
                    f(t);
                }
            }
        }
        _ => {}
    }
}

fn is_placeholder_let(stmt: &Stmt) -> Option<LocalId> {
    match stmt {
        Stmt::Let {
            id,
            init: None | Some(Expr::Undefined),
            ..
        } => Some(*id),
        _ => None,
    }
}

fn count_placeholder_lets(module: &Module) -> Counts {
    let mut c = Counts::new();
    for_each_let(module, &mut |s, _| {
        if let Some(id) = is_placeholder_let(s) {
            bump(&mut c, id);
        }
    });
    c
}

/// CommonJS `var crypto = require('crypto')` bindings that provably hold the
/// `node:crypto` namespace whenever they hold anything but their hoisting
/// placeholder.
///
/// `require` must be the binding perry's CJS wrapper declares
/// (`crates/perry/src/commands/compile/cjs_wrap`): a `const require =
/// function(specifier) { .. }` directly in the body of the closure bound to
/// `__perry_cjs_factory`, which no user code can name. Its builtin arm
/// returns `js_create_native_module_namespace("crypto")`. The alias must have
/// exactly one value-carrying definition — that `require('crypto')` call —
/// and otherwise only `undefined` placeholder declarations: no other `Let`,
/// no `LocalSet`, no `++`/`--`.
fn cjs_crypto_aliases(module: &Module) -> HashSet<LocalId> {
    let mut requires: HashSet<LocalId> = HashSet::new();
    for_each_let(module, &mut |s, _| {
        let Stmt::Let {
            name,
            init: Some(Expr::Closure { body, .. }),
            ..
        } = s
        else {
            return;
        };
        if name != "__perry_cjs_factory" {
            return;
        }
        for inner in body {
            if let Stmt::Let {
                id,
                name,
                mutable: false,
                init: Some(Expr::Closure { params, .. }),
                ..
            } = inner
            {
                if name == "require" && params.len() == 1 && params[0].name == "specifier" {
                    requires.insert(*id);
                }
            }
        }
    });
    if requires.is_empty() {
        return HashSet::new();
    }
    let is_require_crypto = |e: &Expr| {
        matches!(e, Expr::Call { callee, args, .. }
            if matches!(callee.as_ref(), Expr::LocalGet(r) if requires.contains(r))
                && args.len() == 1
                && matches!(&args[0], Expr::String(sp) if sp == "crypto" || sp == "node:crypto"))
    };
    // Value-carrying `Let`s per id, and how many of them are `require('crypto')`.
    let mut lets: HashMap<LocalId, (usize, usize)> = HashMap::new();
    for_each_let(module, &mut |s, _| {
        if let Stmt::Let {
            id, init: Some(e), ..
        } = s
        {
            if !matches!(e, Expr::Undefined) {
                let d = lets.entry(*id).or_default();
                d.0 += 1;
                if is_require_crypto(e) {
                    d.1 += 1;
                }
            }
        }
    });
    // Any `LocalSet`/`++` anywhere in the module disqualifies the binding.
    let writes = count_module(module, true);
    lets.into_iter()
        .filter(|(id, (values, rc))| *values == 1 && *rc == 1 && !writes.contains_key(id))
        .map(|(id, _)| id)
        .collect()
}

fn bump(counts: &mut Counts, id: LocalId) {
    *counts.entry(id).or_insert(0) += 1;
}

/// Count every place a LocalId appears, in every container of the module.
/// Over-counting only makes the pass bail; the container list mirrors
/// `generator::compute_max_local_id` so nothing is under-counted.
fn count_module_occurrences(module: &Module) -> Counts {
    count_module(module, false)
}

/// `writes_only`: count just `LocalSet` / `Update` targets.
fn count_module(module: &Module, writes_only: bool) -> Counts {
    let mut counter = Counter {
        counts: Counts::new(),
        writes_only,
    };
    let c = &mut counter;
    for f in &module.functions {
        count_function(f, c);
    }
    count_stmts(&module.init, c);
    for g in &module.globals {
        c.any(g.id);
        if let Some(init) = &g.init {
            count_expr(init, c);
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
            count_function(f, c);
        }
        for m in &class.computed_members {
            count_expr(&m.key_expr, c);
            count_function(&m.function, c);
        }
        for field in class.fields.iter().chain(&class.static_fields) {
            if let Some(init) = &field.init {
                count_expr(init, c);
            }
            if let Some(key) = &field.key_expr {
                count_expr(key, c);
            }
            count_decorators(&field.decorators, c);
        }
        count_decorators(&class.decorators, c);
        if let Some(e) = &class.extends_expr {
            count_expr(e, c);
        }
    }
    counter.counts
}

struct Counter {
    counts: Counts,
    writes_only: bool,
}

impl Counter {
    /// Any occurrence that is not a write.
    fn any(&mut self, id: LocalId) {
        if !self.writes_only {
            bump(&mut self.counts, id);
        }
    }

    fn write(&mut self, id: LocalId) {
        bump(&mut self.counts, id);
    }
}

fn count_decorators(decorators: &[Decorator], c: &mut Counter) {
    for d in decorators {
        for a in &d.args {
            count_expr(a, c);
        }
    }
}

fn count_params(params: &[Param], c: &mut Counter) {
    for p in params {
        c.any(p.id);
        if let Some(d) = &p.default {
            count_expr(d, c);
        }
        count_decorators(&p.decorators, c);
        if let Some(meta) = &p.arguments_object {
            for (_, local) in &meta.mapped_parameter_ids {
                c.any(*local);
            }
        }
    }
}

fn count_function(f: &Function, c: &mut Counter) {
    count_params(&f.params, c);
    count_decorators(&f.decorators, c);
    for id in &f.captures {
        c.any(*id);
    }
    count_stmts(&f.body, c);
}

fn count_stmts(stmts: &[Stmt], c: &mut Counter) {
    for s in stmts {
        count_stmt(s, c);
    }
}

fn count_stmt(stmt: &Stmt, c: &mut Counter) {
    match stmt {
        Stmt::Let { id, init, .. } => {
            c.any(*id);
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
                    c.any(id);
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
                c.any(*id);
            }
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
    }
}

fn count_expr(expr: &Expr, c: &mut Counter) {
    match expr {
        Expr::LocalGet(id) => c.any(*id),
        Expr::LocalSet(id, _) | Expr::Update { id, .. } => c.write(*id),
        Expr::ArrayPush { array_id, .. }
        | Expr::ArrayPushSpread { array_id, .. }
        | Expr::ArrayUnshift { array_id, .. }
        | Expr::ArraySplice { array_id, .. }
        | Expr::ArrayCopyWithin { array_id, .. } => c.any(*array_id),
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => c.any(*id),
        Expr::SetAdd { set_id, .. } => c.any(*set_id),
        Expr::Closure {
            params,
            body,
            captures,
            mutable_captures,
            ..
        } => {
            // Param defaults are reached by the walker below.
            for p in params {
                c.any(p.id);
                count_decorators(&p.decorators, c);
                if let Some(meta) = &p.arguments_object {
                    for (_, local) in &meta.mapped_parameter_ids {
                        c.any(*local);
                    }
                }
            }
            for id in captures.iter().chain(mutable_captures) {
                c.any(*id);
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
///
/// A CommonJS alias receiver is still read first, as a discarded
/// `crypto.createHash` property get: the alias is either the `require`d
/// namespace or its hoisting placeholder `undefined`, and in the latter case
/// node throws `Cannot read properties of undefined` before evaluating any
/// argument — so does this.
fn rewrite_create(expr: Expr) -> Expr {
    let Expr::Call {
        callee,
        args,
        byte_offset,
        ..
    } = expr
    else {
        unreachable!("checked by create_call_kind")
    };
    let Expr::PropertyGet {
        object,
        property,
        byte_offset: prop_offset,
    } = *callee
    else {
        unreachable!("checked by create_call_kind")
    };
    let init = if matches!(property.as_str(), "createHmac" | "Hmac") {
        CHAIN_INIT_HMAC
    } else {
        CHAIN_INIT_HASH
    };
    let init_call = crypto_call(init, args, byte_offset);
    match *object {
        Expr::NativeModuleRef(_) => init_call,
        receiver => Expr::Sequence(vec![
            Expr::PropertyGet {
                object: Box::new(receiver),
                property,
                byte_offset: prop_offset,
            },
            init_call,
        ]),
    }
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
    aliases: &'a HashSet<LocalId>,
    /// `Let { init: None | Some(Undefined) }` count per id: `var` hoisting
    /// placeholders, which bind no value and so are not an escape.
    placeholders: &'a Counts,
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
        if is_inline_chain(expr, self.aliases)
            && !is_literal_fast_path(expr)
            && !contains_suspension(expr)
        {
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
            if create_call_kind(peel_updates(init), self.aliases).is_none()
                || contains_suspension(init)
            {
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
            // A hoisted `var` also has an `undefined` placeholder declaration.
            let expected = 1 + allowed + self.placeholders.get(&id).copied().unwrap_or(0);
            if allowed == 0 || self.counts.get(&id).copied() != Some(expected) {
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
