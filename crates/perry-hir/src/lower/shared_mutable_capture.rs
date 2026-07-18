//! #5951 — desugar a SHARED-MUTABLE class capture into a one-element array box.
//!
//! Perry lifts a class out of its declaring function and threads captured outer
//! locals through the VALUE-based `__perry_cap_*` snapshot machinery. For an
//! IMMUTABLE capture that is correct. For a MUTABLE one shared between the
//! declaring function and a field-init/method closure it is not: the snapshot
//! hands each side its own copy, so writes on one side are invisible to the
//! other (and to sibling instances).
//!
//! The fix reuses machinery that already shares correctly: a heap **array** is
//! captured by POINTER, not deep-copied, so a value-snapshot of an array
//! preserves identity. We rewrite a detected shared-mutable capture `c` (a
//! scalar local) into a one-element array `c = [<init>]`, and every VALUE
//! read/write of `c` into `c[0]`. The capture site still snapshots `c` — now
//! the array pointer — so the declaring function, every instance, and the
//! closures all read and write the same `c[0]` cell. No change to the (fragile)
//! capture-snapshot codegen is required.
//!
//! Runs AFTER class-capture synthesis (so the per-closure rebind locals named
//! `__perry_cap_<id>` already exist) and BEFORE `widen_mutable_captures` (so
//! the array capture is seen as a by-reference array, not a scalar to box).

use std::collections::{HashMap, HashSet};

use perry_types::{LocalId, Type};

use crate::ir::*;
use crate::walker::{walk_expr_children, walk_expr_children_mut};

/// Collect every locally-assigned id in `stmt`, DESCENDING into closure bodies
/// — unlike `analysis::collect_assigned_locals_stmt`, whose walker stops at
/// closure boundaries. A field-init arrow that does `c += 1` lands its write on
/// the rebind local INSIDE the closure, so the descent is essential to detect
/// class-side mutation of a capture.
fn collect_assigned_deep_stmt(stmt: &Stmt, out: &mut HashSet<LocalId>) {
    for_each_child_stmt(stmt, &mut |s| collect_assigned_deep_stmt(s, out));
    for_each_top_expr(stmt, &mut |e| collect_assigned_deep_expr(e, out));
}

fn collect_assigned_deep_expr(expr: &Expr, out: &mut HashSet<LocalId>) {
    match expr {
        // A `LocalSet(id, ClassCaptureValue{..})` is the capture REBIND, not a
        // real mutation — every captured param/local carries one, so counting
        // it would mark every capture "mutable". Exclude it; real writes
        // (`= expr`, `+= 1`, `++`) still count.
        Expr::LocalSet(id, value) if !matches!(value.as_ref(), Expr::ClassCaptureValue { .. }) => {
            out.insert(*id);
        }
        Expr::Update { id, .. } => {
            out.insert(*id);
        }
        Expr::Closure { body, .. } => {
            for s in body {
                collect_assigned_deep_stmt(s, out);
            }
        }
        _ => {}
    }
    walk_expr_children(expr, &mut |e| collect_assigned_deep_expr(e, out));
}

/// Detect shared-mutable class captures and rewrite them to one-element array
/// boxes. A no-op when there are none (the common case), so non-capturing /
/// immutable-capture code is left byte-identical.

/// Does `name` denote a class-capture field/param for one of `ids`?
/// Matches by parsed outer id (see `crate::cap_fields`): the names carry a
/// per-module salt, and these per-module passes only ever compare names
/// minted by this module's own lowering.
fn is_cap_name_of(name: &str, ids: &HashSet<LocalId>) -> bool {
    crate::cap_fields::cap_field_outer_id(name).is_some_and(|id| ids.contains(&id))
}

pub(crate) fn desugar_shared_mutable_captures(module: &mut Module) {
    // Bisection escape hatch (#5951): disable the desugar to isolate its effect.
    if std::env::var("PERRY_NO_5951").is_ok() {
        return;
    }
    // Module-level bisection gate (#6089 diagnosis): skip the desugar for any
    // module whose name contains one of the comma-separated substrings.
    if let Ok(skips) = std::env::var("PERRY_5951_SKIP_MODS") {
        if skips
            .split(',')
            .filter(|s| !s.is_empty())
            .any(|s| module.name.contains(s))
        {
            return;
        }
    }
    // ---- detection, kept PER BODY ------------------------------------------
    //
    // LocalIds are NOT unique across function scopes (member params/lets restart
    // their id space; sibling functions likewise). The first cut of this pass
    // collected one module-global id set and rewrote EVERY body with it — at
    // Next.js-bundle scale an unrelated local in some other function sharing a
    // flagged numeric id had its reads rewritten to `local[0]`, yielding
    // `undefined` and a uniform `Object.keys(undefined)` 500 on every route
    // (#6089). Detection results and rewrites are now scoped to the body that
    // owns the ids.
    let (mut fn_shared, mut init_shared) = {
        let classes: HashMap<&str, &Class> = module
            .classes
            .iter()
            .map(|c| (c.name.as_str(), c))
            .collect();
        let fn_shared: Vec<HashSet<LocalId>> = module
            .functions
            .iter()
            .map(|f| detect_shared_in_body(&f.body, &classes))
            .collect();
        let init_shared = detect_shared_in_body(&module.init, &classes);
        (fn_shared, init_shared)
    };
    // Keep only ids that are UNAMBIGUOUS within their body (declared exactly
    // once across deep `Let`s + nested closure params — see
    // `retain_unambiguous`). Nested closures restart their id spaces, so a
    // numeric rewrite over the whole body is only sound for unique ids.
    for (f, s) in module.functions.iter().zip(fn_shared.iter_mut()) {
        if s.is_empty() {
            continue;
        }
        let mut counts: HashMap<LocalId, u32> = HashMap::new();
        for p in &f.params {
            *counts.entry(p.id).or_default() += 1;
        }
        for st in &f.body {
            collect_declared_counts_stmt(st, &mut counts);
        }
        retain_unambiguous(s, &counts);
    }
    if !init_shared.is_empty() {
        let mut counts: HashMap<LocalId, u32> = HashMap::new();
        for st in &module.init {
            collect_declared_counts_stmt(st, &mut counts);
        }
        retain_unambiguous(&mut init_shared, &counts);
    }
    let mut all_shared: HashSet<LocalId> = init_shared.iter().copied().collect();
    for s in &fn_shared {
        all_shared.extend(s.iter().copied());
    }
    if all_shared.is_empty() {
        return;
    }

    // ---- declaring bodies: rewrite with ONLY the ids detected in them -------
    for (f, s) in module.functions.iter_mut().zip(fn_shared.iter()) {
        if !s.is_empty() {
            rewrite_stmts(&mut f.body, s, s);
        }
    }
    if !init_shared.is_empty() {
        rewrite_stmts(&mut module.init, &init_shared, &init_shared);
    }

    // ---- lifted class members: per-member rebind ids ------------------------
    //
    // The per-closure rebind locals (`let __perry_cap_<id> = ClassCaptureValue`)
    // and the synthesized ctor params hold the captured pointer — now the array
    // — so their VALUE uses inside the lifted bodies must go through `[0]`.
    // Match them BY NAME within each member and rewrite only that member's body
    // with its own ids (never the declaring `shared` set — the declaring `Let`
    // that gets array-wrapped lives outside the class).
    let targets: &HashSet<LocalId> = &all_shared;
    let no_shared: HashSet<LocalId> = HashSet::new();
    for c in &mut module.classes {
        for m in &mut c.methods {
            rewrite_member_scoped(m, &targets, &no_shared);
        }
        for (_, g) in &mut c.getters {
            rewrite_member_scoped(g, &targets, &no_shared);
        }
        for (_, s) in &mut c.setters {
            rewrite_member_scoped(s, &targets, &no_shared);
        }
        for sm in &mut c.static_methods {
            rewrite_member_scoped(sm, &targets, &no_shared);
        }
        for member in &mut c.computed_members {
            rewrite_member_scoped(&mut member.function, &targets, &no_shared);
        }
        // The constructor and the field initializers share one scope: a
        // field-init closure captures the synthesized CTOR param, so field
        // inits must be rewritten with the ctor-scope ids too.
        let mut ctor_ids: HashSet<LocalId> = HashSet::new();
        if let Some(ctor) = &c.constructor {
            collect_fn_target_ids(ctor, &targets, &mut ctor_ids);
        }
        for f in &c.fields {
            let mut names: HashMap<LocalId, String> = HashMap::new();
            if let Some(init) = &f.init {
                collect_let_names_expr(init, &mut names);
            }
            if let Some(key) = &f.key_expr {
                collect_let_names_expr(key, &mut names);
            }
            for (id, n) in names {
                if is_cap_name_of(&n, targets) {
                    ctor_ids.insert(id);
                }
            }
        }
        if !ctor_ids.is_empty() {
            // Uniqueness over the whole ctor+fields region (one scope).
            let mut counts: HashMap<LocalId, u32> = HashMap::new();
            if let Some(ctor) = &c.constructor {
                for p in &ctor.params {
                    *counts.entry(p.id).or_default() += 1;
                }
                for st in &ctor.body {
                    collect_declared_counts_stmt(st, &mut counts);
                }
            }
            for f in &c.fields {
                if let Some(init) = &f.init {
                    collect_declared_counts_expr(init, &mut counts);
                }
                if let Some(key) = &f.key_expr {
                    collect_declared_counts_expr(key, &mut counts);
                }
            }
            retain_unambiguous(&mut ctor_ids, &counts);
        }
        if !ctor_ids.is_empty() {
            if let Some(ctor) = &mut c.constructor {
                rewrite_stmts(&mut ctor.body, &no_shared, &ctor_ids);
            }
            for f in &mut c.fields {
                if let Some(init) = &mut f.init {
                    rewrite_expr(init, &no_shared, &ctor_ids);
                }
                if let Some(key) = &mut f.key_expr {
                    rewrite_expr(key, &no_shared, &ctor_ids);
                }
            }
        }
    }

    // The capture HOLDERS (constructor param, instance field, per-method rebind
    // `Let`, all named `__perry_cap_<id>`) were typed with the capture's ORIGINAL
    // scalar type but now carry the array pointer. A declared scalar type drives
    // a type-specific representation (e.g. a `string` holder mangles the array
    // handle — #5951 e4). Retype them to `Any` so they use the generic pointer
    // representation, matching the array they now hold.
    if std::env::var("PERRY_5951_NO_RETYPE").is_err() {
        retype_capture_holders(module, &all_shared);
    }
    if std::env::var("PERRY_5951_TRACE").as_deref() == Ok("1") {
        let mut per_fn: Vec<String> = Vec::new();
        for (f, s) in module.functions.iter().zip(fn_shared.iter()) {
            if !s.is_empty() {
                per_fn.push(format!("{}:{:?}", f.name, s));
            }
        }
        if !init_shared.is_empty() {
            per_fn.push(format!("<init>:{init_shared:?}"));
        }
        eprintln!(
            "[5951] module={} desugared {}",
            module.name,
            per_fn.join(" ")
        );
    }
}

/// The ids in `f`'s own scope (params + `Let`s, descending into nested
/// closures) whose NAME is a flagged `__perry_cap_<id>` rebind target.
fn collect_fn_target_ids(f: &Function, targets: &HashSet<LocalId>, out: &mut HashSet<LocalId>) {
    for p in &f.params {
        if is_cap_name_of(&p.name, targets) {
            out.insert(p.id);
        }
    }
    let mut names: HashMap<LocalId, String> = HashMap::new();
    for s in &f.body {
        collect_let_names_stmt(s, &mut names);
    }
    for (id, n) in names {
        if is_cap_name_of(&n, targets) {
            out.insert(id);
        }
    }
}

/// Rewrite one lifted member body with ONLY its own rebind ids — and only
/// those that are UNAMBIGUOUS within the member (declared exactly once across
/// its params and deep `Let`s/closure params; see `retain_unambiguous`).
fn rewrite_member_scoped(
    f: &mut Function,
    targets: &HashSet<LocalId>,
    no_shared: &HashSet<LocalId>,
) {
    let mut ids: HashSet<LocalId> = HashSet::new();
    collect_fn_target_ids(f, targets, &mut ids);
    if ids.is_empty() {
        return;
    }
    let mut counts: HashMap<LocalId, u32> = HashMap::new();
    for p in &f.params {
        *counts.entry(p.id).or_default() += 1;
    }
    for s in &f.body {
        collect_declared_counts_stmt(s, &mut counts);
    }
    retain_unambiguous(&mut ids, &counts);
    if !ids.is_empty() {
        rewrite_stmts(&mut f.body, no_shared, &ids);
    }
}

/// Drop every id that is declared more than once in the rewritten region.
///
/// LocalIds restart per closure scope (#5143 family): inside a CJS module
/// wrapper the whole module body is ONE function whose nested closures reuse
/// low numeric ids constantly. Rewriting `LocalGet(id)` by number is only
/// sound when exactly one binding with that number exists in the region —
/// otherwise an unrelated same-numbered local in a sibling closure would be
/// index-rewritten (`local[0]` on a non-array → `undefined`), which 500'd
/// every route of the Next.js standalone server (#6089). An ambiguous id is
/// skipped: its capture stays a split cell (the lesser, pre-#6054 behavior)
/// instead of corrupting unrelated code.
fn retain_unambiguous(ids: &mut HashSet<LocalId>, counts: &HashMap<LocalId, u32>) {
    if std::env::var("PERRY_5951_TRACE").as_deref() == Ok("1") {
        let dropped: Vec<LocalId> = ids
            .iter()
            .copied()
            .filter(|id| counts.get(id).copied().unwrap_or(0) != 1)
            .collect();
        if !dropped.is_empty() {
            eprintln!("[5951] skipped ambiguous ids {dropped:?}");
        }
    }
    ids.retain(|id| counts.get(id).copied().unwrap_or(0) == 1);
}

/// Count declarations per id across a region: `Let`s plus nested closure
/// PARAMS (which `collect_let_names_*` ignores), descending into closures.
fn collect_declared_counts_stmt(stmt: &Stmt, out: &mut HashMap<LocalId, u32>) {
    if let Stmt::Let { id, .. } = stmt {
        *out.entry(*id).or_default() += 1;
    }
    for_each_child_stmt(stmt, &mut |s| collect_declared_counts_stmt(s, out));
    for_each_top_expr(stmt, &mut |e| collect_declared_counts_expr(e, out));
}

fn collect_declared_counts_expr(expr: &Expr, out: &mut HashMap<LocalId, u32>) {
    if let Expr::Closure { params, body, .. } = expr {
        for p in params {
            *out.entry(p.id).or_default() += 1;
        }
        for s in body {
            collect_declared_counts_stmt(s, out);
        }
    }
    walk_expr_children(expr, &mut |e| collect_declared_counts_expr(e, out));
}

fn retype_capture_holders(module: &mut Module, shared: &HashSet<LocalId>) {
    let targets: &HashSet<LocalId> = shared;
    for c in &mut module.classes {
        for f in &mut c.fields {
            if is_cap_name_of(&f.name, targets) {
                f.ty = Type::Any;
            }
        }
        let mut retype_fn = |f: &mut Function| {
            for p in &mut f.params {
                if is_cap_name_of(&p.name, targets) {
                    p.ty = Type::Any;
                }
            }
            retype_lets_in_stmts(&mut f.body, &targets);
        };
        for m in &mut c.methods {
            retype_fn(m);
        }
        for (_, g) in &mut c.getters {
            retype_fn(g);
        }
        for (_, s) in &mut c.setters {
            retype_fn(s);
        }
        for sm in &mut c.static_methods {
            retype_fn(sm);
        }
        for member in &mut c.computed_members {
            retype_fn(&mut member.function);
        }
        if let Some(ctor) = &mut c.constructor {
            retype_fn(ctor);
        }
    }
}

fn retype_lets_in_stmts(stmts: &mut [Stmt], targets: &HashSet<LocalId>) {
    for s in stmts.iter_mut() {
        retype_lets_in_stmt(s, targets);
    }
}

fn retype_lets_in_stmt(stmt: &mut Stmt, targets: &HashSet<LocalId>) {
    if let Stmt::Let { name, ty, init, .. } = stmt {
        if is_cap_name_of(name, targets) {
            *ty = Type::Any;
        }
        if let Some(e) = init {
            retype_lets_in_expr(e, targets);
        }
    }
    let mut recur = |body: &mut Vec<Stmt>| retype_lets_in_stmts(body, targets);
    match stmt {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            recur(then_branch);
            if let Some(e) = else_branch {
                recur(e);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => recur(body),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                retype_lets_in_stmt(i, targets);
            }
            recur(body);
        }
        Stmt::Labeled { body, .. } => retype_lets_in_stmt(body, targets),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            recur(body);
            if let Some(cc) = catch {
                recur(&mut cc.body);
            }
            if let Some(fin) = finally {
                recur(fin);
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases {
                recur(&mut case.body);
            }
        }
        _ => {}
    }
}

fn retype_lets_in_expr(expr: &mut Expr, targets: &HashSet<LocalId>) {
    if let Expr::Closure { body, .. } = expr {
        retype_lets_in_stmts(body, targets);
    }
    walk_expr_children_mut(expr, &mut |e| retype_lets_in_expr(e, targets));
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// Detect the shared-mutable capture ids declared in ONE body. The returned
/// ids are meaningful only within that body's scope — callers must not apply
/// them to other functions (LocalIds repeat across scopes; see #6089).
fn detect_shared_in_body(body: &[Stmt], classes: &HashMap<&str, &Class>) -> HashSet<LocalId> {
    let mut shared = HashSet::new();
    let mut regs = Vec::new();
    for s in body {
        find_regs_stmt(s, &mut regs);
    }
    if regs.is_empty() {
        return shared;
    }
    let mut assigned: HashSet<LocalId> = HashSet::new();
    for s in body {
        collect_assigned_deep_stmt(s, &mut assigned);
    }
    for (class_name, ids) in regs {
        for id in ids {
            // Declaring-function-side mutation (`c = 99` after `new T()`).
            if assigned.contains(&id) {
                shared.insert(id);
                continue;
            }
            // Class-side mutation: a member assigns rebind local `__perry_cap_<id>`.
            if let Some(c) = classes.get(class_name.as_str()) {
                if class_mutates_capture(c, id) {
                    shared.insert(id);
                }
            }
        }
    }
    shared
}

fn class_mutates_capture(c: &Class, id: LocalId) -> bool {
    let id_name = collect_class_names(c);
    let assigned = collect_class_assigned(c);
    assigned.iter().any(|aid| {
        id_name
            .get(aid)
            .is_some_and(|n| crate::cap_fields::cap_field_outer_id(n) == Some(id))
    })
}

/// id -> name across a class: every member function's PARAMS (a field-init
/// closure captures the constructor param `__perry_cap_<id>`, id-only in the
/// closure) plus every `Let` name in member bodies and field initializers.
fn collect_class_names(c: &Class) -> HashMap<LocalId, String> {
    let mut id_name: HashMap<LocalId, String> = HashMap::new();
    let mut add_fn = |f: &Function, m: &mut HashMap<LocalId, String>| {
        for p in &f.params {
            m.insert(p.id, p.name.clone());
        }
        for s in &f.body {
            collect_let_names_stmt(s, m);
        }
    };
    for m in &c.methods {
        add_fn(m, &mut id_name);
    }
    for (_, g) in &c.getters {
        add_fn(g, &mut id_name);
    }
    for (_, s) in &c.setters {
        add_fn(s, &mut id_name);
    }
    for sm in &c.static_methods {
        add_fn(sm, &mut id_name);
    }
    for member in &c.computed_members {
        add_fn(&member.function, &mut id_name);
    }
    if let Some(ctor) = &c.constructor {
        add_fn(ctor, &mut id_name);
    }
    for f in &c.fields {
        if let Some(init) = &f.init {
            collect_let_names_expr(init, &mut id_name);
        }
        if let Some(key) = &f.key_expr {
            collect_let_names_expr(key, &mut id_name);
        }
    }
    id_name
}

/// Every locally-assigned id across a class (member bodies + field initializers,
/// descending into closures).
fn collect_class_assigned(c: &Class) -> HashSet<LocalId> {
    let mut assigned = HashSet::new();
    for body in class_member_bodies(c) {
        for s in body {
            collect_assigned_deep_stmt(s, &mut assigned);
        }
    }
    for f in &c.fields {
        if let Some(init) = &f.init {
            collect_assigned_deep_expr(init, &mut assigned);
        }
        if let Some(key) = &f.key_expr {
            collect_assigned_deep_expr(key, &mut assigned);
        }
    }
    assigned
}

fn class_member_bodies(c: &Class) -> Vec<&Vec<Stmt>> {
    let mut v: Vec<&Vec<Stmt>> = Vec::new();
    for m in &c.methods {
        v.push(&m.body);
    }
    for (_, g) in &c.getters {
        v.push(&g.body);
    }
    for (_, s) in &c.setters {
        v.push(&s.body);
    }
    for sm in &c.static_methods {
        v.push(&sm.body);
    }
    for member in &c.computed_members {
        v.push(&member.function.body);
    }
    if let Some(ctor) = &c.constructor {
        v.push(&ctor.body);
    }
    v
}

// ---- read-only walkers (exhaustive over Stmt; exprs recurse into closures) --

fn collect_let_names_stmt(stmt: &Stmt, out: &mut HashMap<LocalId, String>) {
    if let Stmt::Let { id, name, .. } = stmt {
        out.insert(*id, name.clone());
    }
    for_each_child_stmt(stmt, &mut |s| collect_let_names_stmt(s, out));
    for_each_top_expr(stmt, &mut |e| collect_let_names_expr(e, out));
}

fn collect_let_names_expr(expr: &Expr, out: &mut HashMap<LocalId, String>) {
    if let Expr::Closure { body, .. } = expr {
        for s in body {
            collect_let_names_stmt(s, out);
        }
    }
    walk_expr_children(expr, &mut |e| collect_let_names_expr(e, out));
}

fn find_regs_stmt(stmt: &Stmt, out: &mut Vec<(String, Vec<LocalId>)>) {
    for_each_child_stmt(stmt, &mut |s| find_regs_stmt(s, out));
    for_each_top_expr(stmt, &mut |e| find_regs_expr(e, out));
}

fn find_regs_expr(expr: &Expr, out: &mut Vec<(String, Vec<LocalId>)>) {
    if let Expr::RegisterClassCaptures {
        class_name,
        captures,
    } = expr
    {
        let ids: Vec<LocalId> = captures
            .iter()
            .filter_map(|c| match c {
                Expr::LocalGet(id) => Some(*id),
                _ => None,
            })
            .collect();
        if !ids.is_empty() {
            out.push((class_name.clone(), ids));
        }
    }
    if let Expr::Closure { body, .. } = expr {
        for s in body {
            find_regs_stmt(s, out);
        }
    }
    walk_expr_children(expr, &mut |e| find_regs_expr(e, out));
}

/// Call `f` on each nested statement body of `stmt` (NOT `stmt` itself).
fn for_each_child_stmt(stmt: &Stmt, f: &mut dyn FnMut(&Stmt)) {
    match stmt {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            then_branch.iter().for_each(&mut *f);
            if let Some(e) = else_branch {
                e.iter().for_each(&mut *f);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => body.iter().for_each(&mut *f),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                f(i);
            }
            body.iter().for_each(&mut *f);
        }
        Stmt::Labeled { body, .. } => f(body),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().for_each(&mut *f);
            if let Some(c) = catch {
                c.body.iter().for_each(&mut *f);
            }
            if let Some(fin) = finally {
                fin.iter().for_each(&mut *f);
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases {
                case.body.iter().for_each(&mut *f);
            }
        }
        _ => {}
    }
}

/// Call `f` on each TOP-LEVEL expression of `stmt` (child exprs handled by the
/// expr walker). Statement bodies are covered by `for_each_child_stmt`.
fn for_each_top_expr(stmt: &Stmt, f: &mut dyn FnMut(&Expr)) {
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
            for case in cases {
                if let Some(t) = &case.test {
                    f(t);
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Rewrite (mutable, exhaustive over Stmt)
// ---------------------------------------------------------------------------

fn rewrite_stmts(stmts: &mut [Stmt], shared: &HashSet<LocalId>, index_uses: &HashSet<LocalId>) {
    for s in stmts.iter_mut() {
        rewrite_stmt(s, shared, index_uses);
    }
}

fn rewrite_stmt(stmt: &mut Stmt, shared: &HashSet<LocalId>, index_uses: &HashSet<LocalId>) {
    match stmt {
        Stmt::Let { id, init, ty, .. } => {
            if let Some(e) = init {
                rewrite_expr(e, shared, index_uses);
            }
            if shared.contains(id) {
                // #6089: an UNINITIALIZED `let prop;` must still become a
                // one-element array — every use is rewritten to `prop[0]`, so
                // leaving `init` as None makes the first write an IndexSet on
                // `undefined` ("Cannot convert undefined or null to object").
                // SWC emits exactly this shape for hoisted computed-property
                // temps (`let prop; class C { static #_ = prop = KEY; }`,
                // next/dist/server/base-http/node.js) — it 500'd every route
                // of the Next.js standalone server at first lazy require.
                let wrapped = match init.take() {
                    Some(e) => e,
                    None => Expr::Undefined,
                };
                *init = Some(Expr::Array(vec![wrapped]));
                *ty = Type::Array(Box::new(ty.clone()));
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => rewrite_expr(e, shared, index_uses),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                rewrite_expr(e, shared, index_uses);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_expr(condition, shared, index_uses);
            rewrite_stmts(then_branch, shared, index_uses);
            if let Some(e) = else_branch {
                rewrite_stmts(e, shared, index_uses);
            }
        }
        Stmt::While { condition, body } => {
            rewrite_expr(condition, shared, index_uses);
            rewrite_stmts(body, shared, index_uses);
        }
        Stmt::DoWhile { body, condition } => {
            rewrite_stmts(body, shared, index_uses);
            rewrite_expr(condition, shared, index_uses);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                rewrite_stmt(i, shared, index_uses);
            }
            if let Some(c) = condition {
                rewrite_expr(c, shared, index_uses);
            }
            if let Some(u) = update {
                rewrite_expr(u, shared, index_uses);
            }
            rewrite_stmts(body, shared, index_uses);
        }
        Stmt::Labeled { body, .. } => rewrite_stmt(body, shared, index_uses),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            rewrite_stmts(body, shared, index_uses);
            if let Some(c) = catch {
                rewrite_stmts(&mut c.body, shared, index_uses);
            }
            if let Some(fin) = finally {
                rewrite_stmts(fin, shared, index_uses);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            rewrite_expr(discriminant, shared, index_uses);
            for case in cases {
                if let Some(t) = &mut case.test {
                    rewrite_expr(t, shared, index_uses);
                }
                rewrite_stmts(&mut case.body, shared, index_uses);
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

fn rewrite_expr(expr: &mut Expr, shared: &HashSet<LocalId>, index_uses: &HashSet<LocalId>) {
    match expr {
        // A value read of a boxed id -> `id[0]`. The synthesized `LocalGet` is
        // the ARRAY handle and is not re-rewritten.
        Expr::LocalGet(id) if index_uses.contains(id) => {
            *expr = Expr::IndexGet {
                object: Box::new(Expr::LocalGet(*id)),
                index: Box::new(Expr::Integer(0)),
            };
            return;
        }
        // Capture REBIND `let/=__perry_cap_N = ClassCaptureValue{..}`: this
        // assigns the WHOLE captured handle (now the array) to the rebind id.
        // Leave it — and its `fallback: LocalGet(id)` — intact.
        Expr::LocalSet(id, value)
            if index_uses.contains(id)
                && matches!(value.as_ref(), Expr::ClassCaptureValue { .. }) =>
        {
            return;
        }
        Expr::LocalSet(id, value) if index_uses.contains(id) => {
            rewrite_expr(value, shared, index_uses);
            let v = std::mem::replace(value.as_mut(), Expr::Undefined);
            *expr = Expr::IndexSet {
                object: Box::new(Expr::LocalGet(*id)),
                index: Box::new(Expr::Integer(0)),
                value: Box::new(v),
            };
            return;
        }
        // Capture STASH `this.__perry_cap_N = <handle>`: keep the whole array
        // handle on the instance field so methods snapshot the shared array.
        Expr::PropertySet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::This) && property.starts_with("__perry_cap_") => {
            return;
        }
        Expr::Update { id, op, prefix } if index_uses.contains(id) => {
            *expr = Expr::IndexUpdate {
                object: Box::new(Expr::LocalGet(*id)),
                index: Box::new(Expr::Integer(0)),
                op: match op {
                    UpdateOp::Increment => BinaryOp::Add,
                    UpdateOp::Decrement => BinaryOp::Sub,
                },
                prefix: *prefix,
            };
            return;
        }
        // Capture sites snapshot the WHOLE handle (the array). Leave the bare
        // `LocalGet(id)` capture args alone; still rewrite non-capture children.
        Expr::RegisterClassCaptures { .. } => return,
        // #6497: the per-evaluation fresh-binding path (#6470) carries the
        // same capture-param-ordered `LocalGet` args as RegisterClassCaptures
        // — they too must snapshot the WHOLE box handle. Rewriting them to
        // `id[0]` stored the cell's current VALUE on the heap class object,
        // so methods of a class that captures AND mutates a local indexed
        // into a number: reads came back `undefined` and writes were lost
        // (gap tests anon_shape_boxed_capture / 5952_mixin_factory_binding).
        // Statics' initializer values are ordinary reads and still rewrite.
        Expr::ClassExprFresh {
            named_statics,
            symbol_statics,
            captured_args,
            ..
        } => {
            for (_, v) in named_statics.iter_mut() {
                rewrite_expr(v, shared, index_uses);
            }
            for (k, v) in symbol_statics.iter_mut() {
                rewrite_expr(k, shared, index_uses);
                rewrite_expr(v, shared, index_uses);
            }
            for a in captured_args.iter_mut() {
                if matches!(a, Expr::LocalGet(id) if index_uses.contains(id)) {
                    continue; // capture argument — keep the array handle
                }
                rewrite_expr(a, shared, index_uses);
            }
            return;
        }
        Expr::New {
            class_name, args, ..
        } => {
            // An OBJECT LITERAL also lowers to `New` — against a synthetic
            // `__AnonShape_*` whose ctor args are the PROPERTY VALUES, not the
            // auto-appended capture handles a real class construction carries.
            // Skipping a bare `LocalGet(boxed)` there stored the one-element array
            // BOX in the field instead of the value it holds, so `{routing: o}`
            // produced `routing === [o]`: every other read of `o` is rewritten to
            // `o[0]` and works, while the object's field holds the cell — the guard
            // `o.locales.length > 1` passes one line before the literal that then
            // hands the callee a `routing` whose `.locales` is `undefined`.
            //
            // A synthetic shape has no captures, so every one of its args is a value
            // and must be rewritten.
            let is_anon_shape = class_name.starts_with("__AnonShape_");
            for a in args.iter_mut() {
                if !is_anon_shape && matches!(a, Expr::LocalGet(id) if index_uses.contains(id)) {
                    continue; // auto-appended capture argument — keep the array handle
                }
                rewrite_expr(a, shared, index_uses);
            }
            return;
        }
        // The closure body is a `Vec<Stmt>` the expr walker does not descend.
        Expr::Closure { body, .. } => {
            rewrite_stmts(body, shared, index_uses);
            // Param defaults are still visited by walk_expr_children_mut below.
        }
        _ => {}
    }
    walk_expr_children_mut(expr, &mut |e| rewrite_expr(e, shared, index_uses));
}
