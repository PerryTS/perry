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

use crate::types::{LocalId, Type};

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

#[derive(Default)]
struct BodySharedCaptures {
    ids: HashSet<LocalId>,
    by_class: HashMap<String, HashSet<LocalId>>,
    census: DeclCensus,
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
        let fn_shared: Vec<BodySharedCaptures> = module
            .functions
            .iter()
            .map(|f| detect_shared_in_body(&f.params, &f.body, &classes))
            .collect();
        let init_shared = detect_shared_in_body(&[], &module.init, &classes);
        (fn_shared, init_shared)
    };
    // Keep only ids that denote ONE binding within their body (see
    // `DeclCensus::is_one_binding`). Nested closures restart their id spaces,
    // so a numeric rewrite over the whole body is only sound for those.
    for shared in fn_shared.iter_mut() {
        if shared.ids.is_empty() {
            continue;
        }
        retain_unambiguous(&mut shared.ids, &shared.census);
        let retained = &shared.ids;
        for ids in shared.by_class.values_mut() {
            ids.retain(|id| retained.contains(id));
        }
        shared.by_class.retain(|_, ids| !ids.is_empty());
    }
    if !init_shared.ids.is_empty() {
        retain_unambiguous(&mut init_shared.ids, &init_shared.census);
        let retained = &init_shared.ids;
        for ids in init_shared.by_class.values_mut() {
            ids.retain(|id| retained.contains(id));
        }
        init_shared.by_class.retain(|_, ids| !ids.is_empty());
    }
    let mut all_shared: HashSet<LocalId> = init_shared.ids.iter().copied().collect();
    for shared in &fn_shared {
        all_shared.extend(shared.ids.iter().copied());
    }
    if all_shared.is_empty() {
        return;
    }
    let mut shared_by_class: HashMap<String, HashSet<LocalId>> = HashMap::new();
    for shared in fn_shared.iter().chain(std::iter::once(&init_shared)) {
        for (class_name, ids) in &shared.by_class {
            shared_by_class
                .entry(class_name.clone())
                .or_default()
                .extend(ids.iter().copied());
        }
    }
    propagate_cells_to_nested_classes(module, &mut shared_by_class);

    // ---- declaring bodies: rewrite with ONLY the ids detected in them -------
    for (f, shared) in module.functions.iter_mut().zip(fn_shared.iter()) {
        let ids = &shared.ids;
        if !ids.is_empty() {
            // Parameters have no `Stmt::Let` for `rewrite_stmt` to wrap. Turn
            // each flagged parameter into the same one-element shared cell at
            // function entry, then let the already-rewritten body use
            // `param[0]`. Add this after rewriting so the initializer's
            // `LocalGet(param)` reads the incoming scalar rather than being
            // rewritten into an index read before the cell exists. Retype the
            // holder to `Any`: its slot now carries an array pointer, not the
            // source parameter's scalar representation.
            let shared_params: Vec<LocalId> = f
                .params
                .iter_mut()
                .filter_map(|param| {
                    if ids.contains(&param.id) {
                        param.ty = Type::Any;
                        Some(param.id)
                    } else {
                        None
                    }
                })
                .collect();
            demote_var_redeclarations(&f.params, &mut f.body, ids);
            rewrite_stmts(&mut f.body, ids, ids);
            for id in shared_params.into_iter().rev() {
                f.body.insert(
                    0,
                    Stmt::Expr(Expr::LocalSet(
                        id,
                        Box::new(Expr::Array(vec![Expr::LocalGet(id)])),
                    )),
                );
            }
        }
    }
    if !init_shared.ids.is_empty() {
        demote_var_redeclarations(&[], &mut module.init, &init_shared.ids);
        rewrite_stmts(&mut module.init, &init_shared.ids, &init_shared.ids);
    }

    // ---- lifted class members: per-member rebind ids ------------------------
    //
    // The per-closure rebind locals (`let __perry_cap_<id> = ClassCaptureValue`)
    // and the synthesized ctor params hold the captured pointer — now the array
    // — so their VALUE uses inside the lifted bodies must go through `[0]`.
    // Match them BY NAME within each member and rewrite only that member's body
    // with its own ids (never the declaring `shared` set — the declaring `Let`
    // that gets array-wrapped lives outside the class).
    let no_shared: HashSet<LocalId> = HashSet::new();
    for c in &mut module.classes {
        let targets = shared_by_class.get(&c.name).unwrap_or(&no_shared);
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
            let mut census = DeclCensus::default();
            let mut walker = CensusWalker::new(&mut census);
            let scope = walker.open_scope();
            if let Some(ctor) = &c.constructor {
                walker.params(&ctor.params, scope);
                walker.stmts(&ctor.body, scope, 0, true);
            }
            for f in &c.fields {
                if let Some(init) = &f.init {
                    walker.expr(init, scope, 0);
                }
                if let Some(key) = &f.key_expr {
                    walker.expr(key, scope, 0);
                }
            }
            retain_unambiguous(&mut ctor_ids, &census);
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
        retype_capture_holders(module, &shared_by_class);
    }
    if std::env::var("PERRY_5951_TRACE").as_deref() == Ok("1") {
        let mut per_fn: Vec<String> = Vec::new();
        for (f, shared) in module.functions.iter().zip(fn_shared.iter()) {
            if !shared.ids.is_empty() {
                per_fn.push(format!("{}:{:?}", f.name, shared.ids));
            }
        }
        if !init_shared.ids.is_empty() {
            per_fn.push(format!("<init>:{:?}", init_shared.ids));
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
/// those that are UNAMBIGUOUS within the member (one binding across its params
/// and deep `Let`s/closure params; see `retain_unambiguous`).
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
    let census = DeclCensus::of_body(&f.params, &f.body);
    retain_unambiguous(&mut ids, &census);
    if !ids.is_empty() {
        rewrite_stmts(&mut f.body, no_shared, &ids);
    }
}

/// Drop every id that does not denote exactly ONE binding in the region.
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
fn retain_unambiguous(ids: &mut HashSet<LocalId>, census: &DeclCensus) {
    if std::env::var("PERRY_5951_TRACE").as_deref() == Ok("1") {
        let dropped: Vec<LocalId> = ids
            .iter()
            .copied()
            .filter(|id| !census.is_one_binding(*id))
            .collect();
        if !dropped.is_empty() {
            eprintln!("[5951] skipped ambiguous ids {dropped:?}");
        }
    }
    ids.retain(|id| census.is_one_binding(*id));
}

/// One declaration of a `LocalId` inside a detection region.
#[derive(Debug)]
struct DeclSite {
    /// Closure scope the declaration lives in (0 = the region's own body).
    scope: u32,
    name: String,
    /// A parameter, or a `Let` that is a direct statement of its scope's body
    /// outside any loop: it runs once, before every later site of that scope.
    dominating: bool,
}

/// Every declaration of every id in a region (params, `Let`s, nested closure
/// params and bodies), in execution order, plus the `var` re-declarations that
/// are real writes to a class-captured binding.
#[derive(Default, Debug)]
struct DeclCensus {
    sites: HashMap<LocalId, Vec<DeclSite>>,
    /// Ids re-declared (`var x = v` after the body-entry `var` slot) at a point
    /// where a class has ALREADY captured the binding, or inside a loop that
    /// can re-run the declaration after a capture. Those declarations are
    /// assignments to a captured binding, exactly like `x = v`.
    late_redeclared: HashSet<LocalId>,
}

impl DeclCensus {
    fn of_body(params: &[Param], body: &[Stmt]) -> Self {
        let mut census = DeclCensus::default();
        let mut walker = CensusWalker::new(&mut census);
        let scope = walker.open_scope();
        walker.params(params, scope);
        walker.stmts(body, scope, 0, true);
        census
    }

    /// Is `id` exactly one binding in this region?
    ///
    /// A single declaration trivially is. Several are one binding only when
    /// they all sit in the SAME closure scope under the SAME name and the first
    /// dominates the rest. That is the shape of a `var`: lowering declares it at
    /// body entry (`predefine_var_bindings_in_function_body`) or reuses a
    /// same-named parameter, and every `var x = v` statement re-declares the
    /// same id (#10485/#10489). Treating those as distinct bindings left every
    /// `var` captured by a class on the value-snapshot path, so class members
    /// never saw later writes and their own writes were lost. Declarations of
    /// one id in different closure scopes stay ambiguous (#6089).
    fn is_one_binding(&self, id: LocalId) -> bool {
        match self.sites.get(&id).map(Vec::as_slice) {
            None | Some([]) => false,
            Some([_]) => true,
            Some([first, rest @ ..]) => {
                first.dominating
                    && rest
                        .iter()
                        .all(|site| site.scope == first.scope && site.name == first.name)
            }
        }
    }
}

/// Execution-order walk that fills a [`DeclCensus`]. Closure bodies open a new
/// scope with a fresh loop depth: each closure invocation gets its own
/// bindings, so an enclosing loop does not re-run a closure-local declaration.
struct CensusWalker<'a> {
    census: &'a mut DeclCensus,
    next_scope: u32,
    /// Ids captured by a class registration seen so far.
    captured: HashSet<LocalId>,
}

impl<'a> CensusWalker<'a> {
    fn new(census: &'a mut DeclCensus) -> Self {
        CensusWalker {
            census,
            next_scope: 0,
            captured: HashSet::new(),
        }
    }

    fn open_scope(&mut self) -> u32 {
        let scope = self.next_scope;
        self.next_scope += 1;
        scope
    }

    fn declare(&mut self, id: LocalId, name: &str, scope: u32, dominating: bool) {
        self.census.sites.entry(id).or_default().push(DeclSite {
            scope,
            name: name.to_string(),
            dominating,
        });
    }

    fn params(&mut self, params: &[Param], scope: u32) {
        for p in params {
            if let Some(default) = &p.default {
                self.expr(default, scope, 0);
            }
            self.declare(p.id, &p.name, scope, true);
        }
    }

    fn stmts(&mut self, stmts: &[Stmt], scope: u32, loop_depth: u32, top: bool) {
        for s in stmts {
            self.stmt(s, scope, loop_depth, top);
        }
    }

    fn stmt(&mut self, stmt: &Stmt, scope: u32, loop_depth: u32, top: bool) {
        match stmt {
            Stmt::Let { id, name, init, .. } => {
                if let Some(e) = init {
                    self.expr(e, scope, loop_depth);
                }
                let redeclaration = self.census.sites.contains_key(id);
                if redeclaration && init.is_some() && (loop_depth > 0 || self.captured.contains(id))
                {
                    self.census.late_redeclared.insert(*id);
                }
                self.declare(*id, name, scope, top && loop_depth == 0);
            }
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => {
                self.expr(e, scope, loop_depth)
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition, scope, loop_depth);
                self.stmts(then_branch, scope, loop_depth, false);
                if let Some(e) = else_branch {
                    self.stmts(e, scope, loop_depth, false);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.expr(condition, scope, loop_depth + 1);
                self.stmts(body, scope, loop_depth + 1, false);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.stmt(i, scope, loop_depth, false);
                }
                if let Some(c) = condition {
                    self.expr(c, scope, loop_depth + 1);
                }
                self.stmts(body, scope, loop_depth + 1, false);
                if let Some(u) = update {
                    self.expr(u, scope, loop_depth + 1);
                }
            }
            Stmt::Labeled { body, .. } => self.stmt(body, scope, loop_depth, false),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.stmts(body, scope, loop_depth, false);
                if let Some(c) = catch {
                    self.stmts(&c.body, scope, loop_depth, false);
                }
                if let Some(fin) = finally {
                    self.stmts(fin, scope, loop_depth, false);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant, scope, loop_depth);
                for case in cases {
                    if let Some(t) = &case.test {
                        self.expr(t, scope, loop_depth);
                    }
                    self.stmts(&case.body, scope, loop_depth, false);
                }
            }
            Stmt::Return(None)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
        }
    }

    fn expr(&mut self, expr: &Expr, scope: u32, loop_depth: u32) {
        match expr {
            Expr::Closure { params, body, .. } => {
                let inner = self.open_scope();
                self.params(params, inner);
                self.stmts(body, inner, 0, true);
                return;
            }
            Expr::RegisterClassCaptures { captures, .. }
            | Expr::ClassExprFresh {
                captured_args: captures,
                ..
            } => {
                for capture in captures {
                    if let Expr::LocalGet(id) = capture {
                        self.captured.insert(*id);
                    }
                }
            }
            _ => {}
        }
        walk_expr_children(expr, &mut |e| self.expr(e, scope, loop_depth));
    }
}

/// Turn every re-declaration of a shared id into a plain assignment, so the
/// rewrite below writes the binding's EXISTING cell (`x[0] = v`) instead of
/// minting a new one that classes captured earlier would never see. A `var x;`
/// re-declaration without an initializer does not touch the binding at all.
/// Only ids `DeclCensus::is_one_binding` accepted reach here, so the first
/// site seen in execution order is the dominating declaration.
fn demote_var_redeclarations(params: &[Param], body: &mut [Stmt], ids: &HashSet<LocalId>) {
    let mut seen: HashSet<LocalId> = params
        .iter()
        .map(|p| p.id)
        .filter(|id| ids.contains(id))
        .collect();
    for s in body.iter_mut() {
        demote_redeclarations_stmt(s, ids, &mut seen);
    }
}

fn demote_redeclarations_stmt(
    stmt: &mut Stmt,
    ids: &HashSet<LocalId>,
    seen: &mut HashSet<LocalId>,
) {
    if let Stmt::Let { id, init, .. } = stmt {
        if let Some(e) = init {
            demote_redeclarations_expr(e, ids, seen);
        }
        if ids.contains(id) && !seen.insert(*id) {
            *stmt = Stmt::Expr(match init.take() {
                Some(value) => Expr::LocalSet(*id, Box::new(value)),
                None => Expr::Undefined,
            });
        }
        return;
    }
    let stmts = |body: &mut [Stmt], seen: &mut HashSet<LocalId>| {
        for s in body.iter_mut() {
            demote_redeclarations_stmt(s, ids, seen);
        }
    };
    match stmt {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => {
            demote_redeclarations_expr(e, ids, seen)
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            demote_redeclarations_expr(condition, ids, seen);
            stmts(then_branch, seen);
            if let Some(e) = else_branch {
                stmts(e, seen);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            demote_redeclarations_expr(condition, ids, seen);
            stmts(body, seen);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                demote_redeclarations_stmt(i, ids, seen);
            }
            if let Some(c) = condition {
                demote_redeclarations_expr(c, ids, seen);
            }
            stmts(body, seen);
            if let Some(u) = update {
                demote_redeclarations_expr(u, ids, seen);
            }
        }
        Stmt::Labeled { body, .. } => demote_redeclarations_stmt(body, ids, seen),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            stmts(body, seen);
            if let Some(c) = catch {
                stmts(&mut c.body, seen);
            }
            if let Some(fin) = finally {
                stmts(fin, seen);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            demote_redeclarations_expr(discriminant, ids, seen);
            for case in cases {
                if let Some(t) = &mut case.test {
                    demote_redeclarations_expr(t, ids, seen);
                }
                stmts(&mut case.body, seen);
            }
        }
        Stmt::Let { .. }
        | Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}

fn demote_redeclarations_expr(
    expr: &mut Expr,
    ids: &HashSet<LocalId>,
    seen: &mut HashSet<LocalId>,
) {
    if let Expr::Closure { params, body, .. } = expr {
        for p in params.iter() {
            if ids.contains(&p.id) {
                seen.insert(p.id);
            }
        }
        for s in body.iter_mut() {
            demote_redeclarations_stmt(s, ids, seen);
        }
    }
    walk_expr_children_mut(expr, &mut |e| demote_redeclarations_expr(e, ids, seen));
}

fn retype_capture_holders(
    module: &mut Module,
    shared_by_class: &HashMap<String, HashSet<LocalId>>,
) {
    let no_shared = HashSet::new();
    for c in &mut module.classes {
        let targets = shared_by_class.get(&c.name).unwrap_or(&no_shared);
        for f in &mut c.fields {
            if is_cap_name_of(&f.name, targets) {
                f.ty = Type::Any;
            }
        }
        let retype_fn = |f: &mut Function| {
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
    let recur = |body: &mut Vec<Stmt>| retype_lets_in_stmts(body, targets);
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
fn detect_shared_in_body(
    params: &[Param],
    body: &[Stmt],
    classes: &HashMap<&str, &Class>,
) -> BodySharedCaptures {
    let mut shared = BodySharedCaptures::default();
    let mut regs = Vec::new();
    for s in body {
        find_regs_stmt(s, &mut regs);
    }
    if regs.is_empty() {
        return shared;
    }
    shared.census = DeclCensus::of_body(params, body);
    let mut assigned: HashSet<LocalId> = HashSet::new();
    for s in body {
        collect_assigned_deep_stmt(s, &mut assigned);
    }
    // A `var x = v` re-declaration that runs after a class captured `x` writes
    // the captured binding just like `x = v` does.
    assigned.extend(shared.census.late_redeclared.iter().copied());
    // Memoized across every (class_name, id) pair checked below (#10757): a
    // class whose own methods construct fresh instances of itself
    // (`new Point(...)` inside a `Point` method, forwarding its own captured
    // context) makes `for_each_nested_capture` report `Point` as "nested
    // inside" `Point`, so `class_mutates_capture` recurses back into the SAME
    // class it started from. That is ordinary self-referential-class code
    // (arithmetic/builder classes returning `new Self(...)`), not a bug in
    // the input — every method of a real point/vector/list class does it.
    // Recomputing the identical (class, id) subproblem at every recursion
    // depth made lowering exponential in the class's method count (bounded
    // only by `MAX_NESTED_CLASS_DEPTH`, so 3-4 self-referencing methods per
    // level already means minutes, not a true non-terminating loop).
    // `mutates_memo` caches a completed (class, id) answer for reuse across
    // every other id/registration that asks the same question;
    // `mutates_visiting` breaks the cycle itself — a (class, id) pair
    // re-entered while still being computed cannot supply new mutation
    // evidence beyond the direct-assignment check already run for it, so a
    // re-entrant call returns `false` without recursing further.
    let mut mutates_memo: HashMap<(String, LocalId), bool> = HashMap::new();
    let mut mutates_visiting: HashSet<(String, LocalId)> = HashSet::new();
    for (class_name, ids) in &regs {
        for id in ids {
            // Declaring-function-side mutation (`c = 99` after `new T()`).
            if assigned.contains(id) {
                shared.ids.insert(*id);
                continue;
            }
            // Class-side mutation: a member assigns rebind local `__perry_cap_<id>`.
            if let Some(c) = classes.get(class_name.as_str()) {
                if class_mutates_capture(classes, c, *id, &mut mutates_memo, &mut mutates_visiting)
                {
                    shared.ids.insert(*id);
                }
            }
        }
    }
    // Every class that captures a boxed id must treat its synthesized holder
    // as the array handle, even if a sibling class is the one that mutates it.
    for (class_name, ids) in regs {
        for id in ids {
            if shared.ids.contains(&id) {
                shared
                    .by_class
                    .entry(class_name.clone())
                    .or_default()
                    .insert(id);
            }
        }
    }
    shared
}

/// Does class `c` (or a class NESTED in one of its member bodies) assign the
/// capture of `id`?
///
/// A member's own write lands on its rebind local `__perry_cap_<id>`. A class
/// declared inside that member body (`class Outer { make() { return class Inner
/// { constructor() { n++ } } } }`, the emscripten/`FS` shape) captures the
/// REBIND local instead, and its write lands one level deeper — invisible to a
/// walk of `c` alone, because a nested class's members live in their own
/// `module.classes` entry, not inside the method body (#10489).
///
/// `for_each_nested_capture` (below) also fires for a class that constructs a
/// fresh instance of ITSELF from inside one of its own methods (`new
/// Point(...)` inside `Point`, forwarding the same captured context) — an
/// everyday shape for arithmetic/builder classes, not an actual nested
/// declaration. That makes `c` reachable from itself, so `memo`/`visiting`
/// (owned by the caller, threaded through every recursive call and shared
/// across every `(class, id)` pair `detect_shared_in_body` asks about) are
/// required for termination in bounded time, not just an optimization: a
/// class whose methods each self-construct turns every recursive step into a
/// full re-scan of `c`, making the naive walk exponential in `c`'s method
/// count (#10757 — a real `weierstrassPoints()`-shaped elliptic-curve `Point`
/// class from `@noble/curves` took over a minute to lower a single module).
/// `visiting` also replaces the old hardcoded recursion-depth cap: a
/// `(class, id)` pair re-entered while its own computation is still on the
/// stack cannot supply mutation evidence beyond the direct-assignment check
/// already running for it, so re-entry returns `false` immediately instead of
/// recursing — correct for a genuine cycle (mutual self/sibling construction)
/// and, unlike a depth cap, never bounds out on a long-but-acyclic chain.
fn class_mutates_capture(
    classes: &HashMap<&str, &Class>,
    c: &Class,
    id: LocalId,
    memo: &mut HashMap<(String, LocalId), bool>,
    visiting: &mut HashSet<(String, LocalId)>,
) -> bool {
    let key = (c.name.clone(), id);
    if let Some(&cached) = memo.get(&key) {
        return cached;
    }
    if !visiting.insert(key.clone()) {
        return false;
    }
    let id_name = collect_class_names(c);
    let assigned = collect_class_assigned(c);
    let names_id = |aid: &LocalId| {
        id_name
            .get(aid)
            .is_some_and(|n| crate::cap_fields::cap_field_outer_id(n) == Some(id))
    };
    let result = if assigned.iter().any(names_id) {
        true
    } else {
        for_each_nested_capture(c, &HashSet::from([id]), |nested_name, outer_id| {
            classes.get(nested_name).is_some_and(|nested| {
                class_mutates_capture(classes, nested, outer_id, &mut *memo, &mut *visiting)
            })
        })
    };
    visiting.remove(&key);
    memo.insert(key, result);
    result
}

/// How far `propagate_cells_to_nested_classes`'s fixpoint iterates (each round
/// is one class-nesting level; unrelated to `class_mutates_capture`, which
/// terminates via `memo`/`visiting` instead of a depth bound). Deep enough for
/// real code, bounded so a cyclic registration cannot loop.
const MAX_NESTED_CLASS_DEPTH: u32 = 8;

/// Call `visit(nested_class_name, outer_id)` for every class registered inside
/// a member body of `c` that captures that member's rebind local for `outer_id`
/// (an id in `targets`). Returns true as soon as `visit` does.
///
/// The nested class was lowered BEFORE `synthesize_class_captures` renamed the
/// enclosing member's references, so its own rebind holders are still named for
/// the ORIGINAL outer id — which is what the reported id must be for the
/// name-keyed matching in `rewrite_member_scoped` / `retype_capture_holders`.
fn for_each_nested_capture(
    c: &Class,
    targets: &HashSet<LocalId>,
    mut visit: impl FnMut(&str, LocalId) -> bool,
) -> bool {
    for f in class_member_fns(c) {
        let mut rebinds = member_rebind_targets(f, targets);
        let mut regs = Vec::new();
        for s in &f.body {
            find_regs_stmt(s, &mut regs);
            find_cap_arg_news_stmt(s, &mut regs);
        }
        // A field initializer shares the constructor's scope (it is lowered
        // into the ctor body), so a class declared in one holds the ctor's
        // rebind params.
        if c.constructor.as_ref().is_some_and(|ctor| ctor.id == f.id) {
            for field in &c.fields {
                for expr in field.init.iter().chain(field.key_expr.iter()) {
                    let mut names: HashMap<LocalId, String> = HashMap::new();
                    collect_let_names_expr(expr, &mut names);
                    for (id, name) in names {
                        if let Some(outer) = crate::cap_fields::cap_field_outer_id(&name) {
                            if targets.contains(&outer) {
                                rebinds.insert(id, outer);
                            }
                        }
                    }
                    find_regs_expr(expr, &mut regs);
                    find_cap_arg_news_expr(expr, &mut regs);
                }
            }
        }
        if rebinds.is_empty() {
            continue;
        }
        for (nested_name, ids) in regs {
            for id in ids {
                if let Some(outer_id) = rebinds.get(&id) {
                    if visit(&nested_name, *outer_id) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Class constructions whose trailing `cap_args_appended` arguments forward
/// capture handles. An IMMEDIATELY constructed class expression (`new (class {
/// … })()`) has neither a `RegisterClassCaptures` nor a `ClassExprFresh` node —
/// `lower_new` lowers it straight to `Expr::New` — so `find_regs_stmt` alone
/// misses it, and its members kept reading the raw cell (#10485's nested-class
/// row printed `[[2],["set"]]` instead of the values).
fn find_cap_arg_news_stmt(stmt: &Stmt, out: &mut Vec<(String, Vec<LocalId>)>) {
    for_each_child_stmt(stmt, &mut |s| find_cap_arg_news_stmt(s, out));
    for_each_top_expr(stmt, &mut |e| find_cap_arg_news_expr(e, out));
}

fn find_cap_arg_news_expr(expr: &Expr, out: &mut Vec<(String, Vec<LocalId>)>) {
    if let Expr::New {
        class_name,
        args,
        cap_args_appended,
        ..
    } = expr
    {
        let appended = *cap_args_appended as usize;
        if appended > 0 && args.len() >= appended {
            let ids: Vec<LocalId> = args[args.len() - appended..]
                .iter()
                .filter_map(|a| match a {
                    Expr::LocalGet(id) => Some(*id),
                    _ => None,
                })
                .collect();
            if !ids.is_empty() {
                out.push((class_name.clone(), ids));
            }
        }
    }
    if let Expr::Closure { body, .. } = expr {
        for s in body {
            find_cap_arg_news_stmt(s, out);
        }
    }
    walk_expr_children(expr, &mut |e| find_cap_arg_news_expr(e, out));
}

/// The member's capture holders (`__perry_cap_<outer>` params and `Let`s),
/// mapped back to the outer id each one rebinds.
fn member_rebind_targets(f: &Function, targets: &HashSet<LocalId>) -> HashMap<LocalId, LocalId> {
    let mut rebinds: HashMap<LocalId, LocalId> = HashMap::new();
    let record = |id: LocalId, name: &str, out: &mut HashMap<LocalId, LocalId>| {
        if let Some(outer) = crate::cap_fields::cap_field_outer_id(name) {
            if targets.contains(&outer) {
                out.insert(id, outer);
            }
        }
    };
    for p in &f.params {
        record(p.id, &p.name, &mut rebinds);
    }
    let mut names: HashMap<LocalId, String> = HashMap::new();
    for s in &f.body {
        collect_let_names_stmt(s, &mut names);
    }
    for (id, n) in names {
        record(id, &n, &mut rebinds);
    }
    rebinds
}

/// A class nested in a member body holds its captures through the member's
/// REBIND locals, which by then carry the shared cell — so ITS members must
/// index through `[0]` too. Walk the nesting chain and mark those classes.
fn propagate_cells_to_nested_classes(
    module: &Module,
    shared_by_class: &mut HashMap<String, HashSet<LocalId>>,
) {
    for _ in 0..MAX_NESTED_CLASS_DEPTH {
        let mut discovered: Vec<(String, LocalId)> = Vec::new();
        for c in &module.classes {
            let Some(targets) = shared_by_class.get(&c.name) else {
                continue;
            };
            for_each_nested_capture(c, targets, |nested_name, outer_id| {
                discovered.push((nested_name.to_string(), outer_id));
                false
            });
        }
        let mut added = false;
        for (class_name, id) in discovered {
            added |= shared_by_class.entry(class_name).or_default().insert(id);
        }
        if !added {
            break;
        }
    }
}

/// Every member function of a class: methods, accessors, statics, computed
/// members and the constructor (whose scope the field initializers share).
fn class_member_fns(c: &Class) -> Vec<&Function> {
    let mut v: Vec<&Function> = Vec::new();
    v.extend(c.methods.iter());
    v.extend(c.getters.iter().map(|(_, g)| g));
    v.extend(c.setters.iter().map(|(_, s)| s));
    v.extend(c.static_methods.iter());
    v.extend(c.computed_members.iter().map(|m| &m.function));
    v.extend(c.constructor.iter());
    v
}

/// id -> name across a class: every member function's PARAMS (a field-init
/// closure captures the constructor param `__perry_cap_<id>`, id-only in the
/// closure) plus every `Let` name in member bodies and field initializers.
fn collect_class_names(c: &Class) -> HashMap<LocalId, String> {
    let mut id_name: HashMap<LocalId, String> = HashMap::new();
    let add_fn = |f: &Function, m: &mut HashMap<LocalId, String>| {
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
    for f in class_member_fns(c) {
        for s in &f.body {
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
    let registration = match expr {
        Expr::RegisterClassCaptures {
            class_name,
            captures,
        } => Some((class_name, captures)),
        // A fresh class expression carries the same capture vector as a
        // declaration snapshot, but it deliberately has no
        // `RegisterClassCaptures`: each evaluation stores its environment on
        // its own heap class object. Treat that vector as a registration for
        // shared-mutable detection too. Otherwise a mutation nested in a
        // fresh class member (for example a defineProperty setter created by
        // a static method) receives a private scalar copy while sibling
        // methods keep reading the class object's stale capture value.
        Expr::ClassExprFresh {
            template,
            captured_args,
            ..
        } => Some((template, captured_args)),
        _ => None,
    };
    if let Some((class_name, captures)) = registration {
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

/// `Sequence([LocalSet(id, _) | Update { id }, this.__perry_cap_N = LocalGet(id)])`
/// for a cell id — see the `Expr::Sequence` arm of [`rewrite_expr`].
fn is_redundant_cell_propagation(items: &[Expr], index_uses: &HashSet<LocalId>) -> bool {
    let [write, Expr::PropertySet {
        object,
        property,
        value,
    }] = items
    else {
        return false;
    };
    let written = match write {
        Expr::LocalSet(id, _) | Expr::Update { id, .. } => *id,
        _ => return false,
    };
    index_uses.contains(&written)
        && matches!(object.as_ref(), Expr::This)
        && property.starts_with("__perry_cap_")
        && matches!(value.as_ref(), Expr::LocalGet(id) if *id == written)
}

/// The class-environment twin of [`is_redundant_cell_propagation`]:
/// `Sequence([LocalSet(id, _) | Update { id }, ClassEnvSet { value: LocalGet(id) }])`.
fn is_redundant_env_propagation(items: &[Expr], index_uses: &HashSet<LocalId>) -> bool {
    let [write, Expr::ClassEnvSet { value, .. }] = items else {
        return false;
    };
    let written = match write {
        Expr::LocalSet(id, _) | Expr::Update { id, .. } => *id,
        _ => return false,
    };
    index_uses.contains(&written) && matches!(value.as_ref(), Expr::LocalGet(id) if *id == written)
}

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
            // A lexical classic-for head has a distinct binding on every
            // iteration.  Its `Stmt::Let` is the one source-level binding that
            // lives in `For::init`; `var` heads are hoisted ahead of the loop
            // and leave this slot empty.  Remember a shared capture before
            // rewriting the Let into its one-element cell.
            let per_iteration_capture = init.as_deref().and_then(|stmt| match stmt {
                Stmt::Let { id, .. } if shared.contains(id) => Some(*id),
                _ => None,
            });
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

            if let Some(id) = per_iteration_capture {
                // CreatePerIterationEnvironment copies the current lexical
                // value before evaluating the update expression.  The shared-
                // mutable class-capture representation needs the equivalent
                // operation at the cell level: replace the loop local with a
                // fresh `[old[0]]` cell, leaving classes from the completed
                // iteration attached to the old cell.  Insert this AFTER the
                // ordinary rewrite so the whole-cell LocalSet is not itself
                // changed into an IndexSet.
                let freshen = Expr::LocalSet(
                    id,
                    Box::new(Expr::Array(vec![Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(id)),
                        index: Box::new(Expr::Integer(0)),
                    }])),
                );
                *update = Some(match update.take() {
                    Some(original) => Expr::Sequence(vec![freshen, original]),
                    None => freshen,
                });
            }
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
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}

fn rewrite_expr(expr: &mut Expr, shared: &HashSet<LocalId>, index_uses: &HashSet<LocalId>) {
    match expr {
        // A member's write to a captured local arrives wrapped by the field
        // propagation of `synthesize_class_captures`:
        // `Sequence([write, this.__perry_cap_N = LocalGet(rebind)])`, which
        // keeps a value SNAPSHOT field in step with the member's local. A
        // shared cell needs no propagation — the field already holds the same
        // cell — and keeping it makes the sequence yield the cell handle instead
        // of the write's value (`return n++` returned `[3]`, not 2; #10489).
        Expr::Sequence(items)
            if is_redundant_cell_propagation(items, index_uses)
                || is_redundant_env_propagation(items, index_uses) =>
        {
            let write = items.swap_remove(0);
            *expr = write;
            rewrite_expr(expr, shared, index_uses);
            return;
        }
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
        // Constructor PUBLISH `ClassEnvSet { value: LocalGet(param) }`: the
        // environment holds the whole array handle, exactly like the instance
        // stash above. Any other value is an ordinary expression.
        Expr::ClassEnvSet { value, .. } if matches!(value.as_ref(), Expr::LocalGet(id) if index_uses.contains(id)) =>
        {
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
                strict: false,
            };
            return;
        }
        // Capture sites snapshot the WHOLE handle (the array). Leave the bare
        // `LocalGet(id)` capture args alone; still rewrite non-capture children.
        Expr::RegisterClassCaptures { .. } => return,
        // Release follow-up: the end-of-body refresh for a fresh class
        // object carries the same capture-param-ordered handles as the initial
        // `ClassExprFresh` snapshot. A shared-mutable capture is a one-element
        // array cell, so refreshing with `id[0]` replaces the cell with its
        // current scalar value. The constructor still treats the refreshed
        // slot as a cell and reads `[0]`, producing `undefined`. Preserve bare
        // shared-cell handles in `captures`, while still rewriting the owner
        // expression and any non-capture children normally.
        Expr::RefreshClassExprCaptures {
            class_value,
            captures,
            ..
        } => {
            rewrite_expr(class_value, shared, index_uses);
            for capture in captures.iter_mut() {
                if matches!(capture, Expr::LocalGet(id) if index_uses.contains(id)) {
                    continue;
                }
                rewrite_expr(capture, shared, index_uses);
            }
            return;
        }
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
            computed_keys,
            computed_statics,
            captured_args,
            ..
        } => {
            for (_, v) in named_statics.iter_mut() {
                rewrite_expr(v, shared, index_uses);
            }
            for (_, key) in computed_keys.iter_mut() {
                rewrite_expr(key, shared, index_uses);
            }
            for (_, v) in computed_statics.iter_mut() {
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
        //
        // A closure nested in a module/function body owns its own parameter
        // scope. When a parameter is the shared class capture, rewriting its
        // reads to `param[0]` is only valid after replacing the incoming value
        // with the one-element cell at closure entry. The top-level Function
        // path above already did this, but nested arrow/function expressions
        // did not: `const make = (options?) => { class C { m() { return
        // options.x } } }` dereferenced `undefined[0]` before optional chaining
        // could short-circuit. Mirror the Function treatment here and insert
        // the wrapper only after rewriting the original body, so its initializer
        // remains the incoming scalar value rather than `param[0]`.
        Expr::Closure { params, body, .. } => {
            let shared_params: Vec<LocalId> = params
                .iter_mut()
                .filter_map(|param| {
                    if index_uses.contains(&param.id) {
                        param.ty = Type::Any;
                        Some(param.id)
                    } else {
                        None
                    }
                })
                .collect();
            rewrite_stmts(body, shared, index_uses);
            for id in shared_params.into_iter().rev() {
                body.insert(
                    0,
                    Stmt::Expr(Expr::LocalSet(
                        id,
                        Box::new(Expr::Array(vec![Expr::LocalGet(id)])),
                    )),
                );
            }
            // Param defaults are still visited by walk_expr_children_mut below.
        }
        _ => {}
    }
    walk_expr_children_mut(expr, &mut |e| rewrite_expr(e, shared, index_uses));
}
