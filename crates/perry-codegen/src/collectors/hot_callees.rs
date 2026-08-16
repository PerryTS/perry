//! Whole-module pre-pass that finds user functions eligible for the
//! inline-hot-small heuristic: **small** functions that are called from a
//! **loop** and have **few total call sites**.
//!
//! Why all three conditions (see `codegen/function.rs` for the size gate, and
//! `linker.rs` for the raised `-inlinehint-threshold`):
//!
//! * **In a loop** (approx. "hot", since AOT has no profile) — a function only
//!   ever called from straight-line / cold code is never hinted, so cold
//!   utility functions can't bloat the binary.
//! * **Few call sites** — this is the anti-bloat backstop. `inlinehint` +
//!   the raised threshold lifts LLVM's inline ceiling for the callee at *every*
//!   one of its call sites (LLVM can't tell hot from cold per-site through a
//!   function attribute). A small function called from 1 hot loop **and** 300
//!   cold sites would therefore be duplicated 300×. Capping total call sites
//!   bounds the duplication: a hinted function is inlined at most
//!   `max_call_sites` times, so the added code is bounded regardless of the
//!   raised threshold. A bit-mixer kernel like `mix` has 1 HIR call site, so it
//!   qualifies; a shared helper called from dozens of sites does not.
//!
//! Direction of error: **under-inclusion is safe** (a missed call site just
//! forgoes an inlining opportunity or, via undercount, leaves a function
//! eligible when it has slightly more sites than counted — still bounded). We
//! never propagate the in-loop flag into a nested closure body (the closure
//! runs at its own, unknown frequency), so we can't wrongly mark a cold callee
//! hot. The walker covers the common expression containers and uses a
//! non-recursing catch-all for the long tail of runtime-intrinsic variants.

use std::collections::{HashMap, HashSet};

use perry_hir::{CallArg, Expr, Module, Stmt};

#[derive(Default)]
struct HotCalleeScan {
    /// FuncIds with ≥1 direct call site (`FuncRef` callee) inside a loop.
    in_loop: HashSet<u32>,
    /// Total direct call-site count per FuncId (loop and non-loop).
    call_counts: HashMap<u32, u32>,
    /// Whether any loop calls through a value whose FuncId is unknown here.
    indirect_call_in_loop: bool,
}

/// #7908: indirect calls do not provide a cheap points-to proof, so admission
/// is all-or-none and capped on the cost-bearing unit: allocation sites in
/// closure bodies. At the measured ~268 bytes per inline bump this bounds a
/// module's speculative growth to ~2.1 KiB.
const INDIRECT_CLOSURE_ALLOC_SITE_BUDGET: u32 = 8;

/// Collect the set of `FuncId`s eligible for `inlinehint`: those with ≥1 direct
/// call site inside a loop AND at most `max_call_sites` total direct call sites
/// across the whole module (`init` + every function + every executable
/// class-member body: constructor, instance/static methods, getters/setters,
/// computed-key members, and instance/static field initializers).
///
/// Counting *every* call site matters for the anti-bloat cap, not just for
/// finding hot callees: `max_call_sites` bounds duplication, so a call site the
/// scan misses is a call site the cap can't see — a function with many real
/// call sites hidden in (say) a getter or a field initializer could slip under
/// the cap and get hinted despite being widely used. Scanning all member bodies
/// keeps the count accurate so the cap stays a true upper bound.
pub fn collect_hot_loop_callees(hir: &Module, max_call_sites: u32) -> HashSet<u32> {
    let mut scan = HotCalleeScan::default();
    walk_stmts(&hir.init, false, &mut scan);
    for f in &hir.functions {
        walk_stmts(&f.body, false, &mut scan);
    }
    for c in &hir.classes {
        if let Some(ctor) = &c.constructor {
            walk_stmts(&ctor.body, false, &mut scan);
        }
        // Instance methods, static methods, and instance/static accessors all
        // carry ordinary statement bodies. (Static accessors live in `getters`
        // / `setters` alongside instance accessors — see `Class` docs — so
        // scanning those covers them.)
        for m in c.methods.iter().chain(c.static_methods.iter()) {
            walk_stmts(&m.body, false, &mut scan);
        }
        for (_, g) in &c.getters {
            walk_stmts(&g.body, false, &mut scan);
        }
        for (_, s) in &c.setters {
            walk_stmts(&s.body, false, &mut scan);
        }
        // Computed-key members: the member body *and* its key expression (the
        // key is evaluated at class-declaration time and can hold a call).
        for cm in &c.computed_members {
            walk_expr(&cm.key_expr, false, &mut scan);
            walk_stmts(&cm.function.body, false, &mut scan);
        }
        // Instance + static field initializers (and any computed key exprs).
        // These run once per construction / at class-eval time, so they stay
        // `in_loop = false`: they contribute to the call-site *count* (feeding
        // the cap) without spuriously marking a callee hot.
        for field in c.fields.iter().chain(c.static_fields.iter()) {
            if let Some(key) = &field.key_expr {
                walk_expr(key, false, &mut scan);
            }
            if let Some(init) = &field.init {
                walk_expr(init, false, &mut scan);
            }
        }
    }
    scan.in_loop
        .iter()
        .copied()
        .filter(|id| scan.call_counts.get(id).copied().unwrap_or(0) <= max_call_sites)
        .collect()
}

/// #7871: the set of `FuncId`s whose bodies should be treated as **hot enough
/// to inline the bump allocator** at their `new` sites
/// (`lower_call/new_alloc.rs::new_site_is_in_loop`).
///
/// ## Why this is not [`collect_hot_loop_callees`]
///
/// It is the same "is this code hot" question **without the
/// `max_call_sites` cap**, because the cap answers a question the allocator
/// does not ask. `inlinehint` duplicates the whole callee body at every one of
/// its call sites, so its cost scales with call-site count and the cap is the
/// only thing bounding it. The inline bump allocator emits ~268 bytes **per
/// `new` site in the function itself**, once, whatever the call-site count —
/// so capping on call sites prices a cost that does not exist and, worse,
/// excludes precisely the functions that earn it.
///
/// `gc-handoff/apps/interp.ts`'s `evalNode` is the shape: it is *the* hot
/// function of the program (~20M invocations), it allocates a `Value` per
/// invocation, and it has 11 direct call sites — 10 of them its own recursion —
/// so the ≤4 cap excluded it and all eight of its object literals took the
/// outlined `js_object_alloc_class_inline_keys` call. Measured on the whole
/// 19-program corpus with `PERRY_INLINE_NEW=1` (which forces the inline form
/// everywhere): `interp` −16.2%, `iso_miss` −10.4%, `pipeline` −8.4%, and the
/// other 16 within a ±1.6% noise floor established by the 15 binaries that
/// come out byte-identical.
///
/// ## The three admission rules
///
/// 1. **≥1 direct call site inside a loop** — the existing proxy for "runs many
///    times", now uncapped.
/// 2. **Directly self-recursive** — a function that calls itself IS a loop, and
///    the existing lexical test cannot see it. `parseExpr`/`evalNode` are both;
///    a recursive descent whose entry call happens to sit in straight-line code
///    would otherwise pay the outlined allocator at every level of the
///    recursion.
/// 3. **Bounded closure module with an indirect loop call** — an indirect call
///    through a local/array value does not expose the target FuncId, as in
///    `pipeline`'s `stage = stages[s]; stage(rec)`. If the module contains such
///    a call, admit its allocation-bearing closures only when they contain at
///    most [`INDIRECT_CLOSURE_ALLOC_SITE_BUDGET`] `new` sites in total. The
///    all-or-none cap avoids traversal-order-dependent code size and prices the
///    actual emitted cost rather than closure count.
///
/// Direction of error is unchanged from the sibling: under-inclusion forgoes
/// speed, never correctness — the outlined call performs the identical bump
/// alloc + header init and returns the identical user pointer.
pub fn collect_alloc_hot_functions(hir: &Module) -> HashSet<u32> {
    let mut scan = HotCalleeScan::default();
    walk_stmts(&hir.init, false, &mut scan);
    for f in &hir.functions {
        walk_stmts(&f.body, false, &mut scan);
    }
    for c in &hir.classes {
        if let Some(ctor) = &c.constructor {
            walk_stmts(&ctor.body, false, &mut scan);
        }
        for m in c.methods.iter().chain(c.static_methods.iter()) {
            walk_stmts(&m.body, false, &mut scan);
        }
        for (_, g) in &c.getters {
            walk_stmts(&g.body, false, &mut scan);
        }
        for (_, s) in &c.setters {
            walk_stmts(&s.body, false, &mut scan);
        }
        for cm in &c.computed_members {
            walk_expr(&cm.key_expr, false, &mut scan);
            walk_stmts(&cm.function.body, false, &mut scan);
        }
        for field in c.fields.iter().chain(c.static_fields.iter()) {
            if let Some(key) = &field.key_expr {
                walk_expr(key, false, &mut scan);
            }
            if let Some(init) = &field.init {
                walk_expr(init, false, &mut scan);
            }
        }
    }
    let mut hot = scan.in_loop;
    // Rule 2: a direct self-call. Scanned per function so the recursion is
    // attributed to the caller it actually appears in, which a whole-module
    // call-count table cannot express.
    for f in &hir.functions {
        let mut self_scan = HotCalleeScan::default();
        walk_stmts(&f.body, false, &mut self_scan);
        if self_scan.call_counts.contains_key(&f.id) {
            hot.insert(f.id);
        }
    }
    // Rule 3: the call site proves hotness but not identity. Speculate only
    // when every allocation-bearing closure in the module fits the fixed byte
    // budget; otherwise admit none of them.
    let closure_alloc_sites = collect_closure_alloc_sites(hir);
    let closure_alloc_site_count = closure_alloc_sites
        .values()
        .copied()
        .fold(0_u32, u32::saturating_add);
    if scan.indirect_call_in_loop
        && closure_alloc_site_count > 0
        && closure_alloc_site_count <= INDIRECT_CLOSURE_ALLOC_SITE_BUDGET
    {
        hot.extend(
            closure_alloc_sites
                .iter()
                .filter_map(|(&func_id, &sites)| (sites > 0).then_some(func_id)),
        );
    }
    hot
}

fn record_callee(callee: &Expr, in_loop: bool, scan: &mut HotCalleeScan) {
    if let Expr::FuncRef(id) = callee {
        *scan.call_counts.entry(*id).or_insert(0) += 1;
        if in_loop {
            scan.in_loop.insert(*id);
        }
    } else if in_loop && !matches!(callee, Expr::ExternFuncRef { .. }) {
        // ExternFuncRef is statically identified even though it has no local
        // FuncId. Every other generic Call callee is a runtime value here.
        scan.indirect_call_in_loop = true;
    }
}

fn walk_stmts(stmts: &[Stmt], in_loop: bool, scan: &mut HotCalleeScan) {
    for s in stmts {
        walk_stmt(s, in_loop, scan);
    }
}

fn walk_stmt(s: &Stmt, in_loop: bool, scan: &mut HotCalleeScan) {
    match s {
        Stmt::Let { init: Some(e), .. } => walk_expr(e, in_loop, scan),
        Stmt::Let { init: None, .. } => {}
        Stmt::Expr(e) | Stmt::Throw(e) => walk_expr(e, in_loop, scan),
        Stmt::Return(Some(e)) => walk_expr(e, in_loop, scan),
        Stmt::Return(None) => {}
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            walk_expr(condition, in_loop, scan);
            walk_stmts(then_branch, in_loop, scan);
            if let Some(eb) = else_branch {
                walk_stmts(eb, in_loop, scan);
            }
        }
        // The dominant guard: everything inside a loop body / its per-iteration
        // condition + update is "hot". The `for`-init runs once, so it keeps
        // the enclosing `in_loop`.
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            walk_expr(condition, true, scan);
            walk_stmts(body, true, scan);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                walk_stmt(init_stmt, in_loop, scan);
            }
            if let Some(c) = condition {
                walk_expr(c, true, scan);
            }
            if let Some(u) = update {
                walk_expr(u, true, scan);
            }
            walk_stmts(body, true, scan);
        }
        Stmt::Labeled { body, .. } => walk_stmt(body, in_loop, scan),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            walk_stmts(body, in_loop, scan);
            if let Some(c) = catch {
                walk_stmts(&c.body, in_loop, scan);
            }
            if let Some(f) = finally {
                walk_stmts(f, in_loop, scan);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            walk_expr(discriminant, in_loop, scan);
            for c in cases {
                if let Some(t) = &c.test {
                    walk_expr(t, in_loop, scan);
                }
                walk_stmts(&c.body, in_loop, scan);
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

fn walk_call_args(args: &[CallArg], in_loop: bool, scan: &mut HotCalleeScan) {
    for a in args {
        match a {
            CallArg::Expr(e) | CallArg::Spread(e) => walk_expr(e, in_loop, scan),
        }
    }
}

fn walk_expr(e: &Expr, in_loop: bool, scan: &mut HotCalleeScan) {
    match e {
        // The two forms that resolve to a known local function. Record the
        // callee (count + in-loop flag), then keep descending — args can
        // themselves contain calls / closures.
        Expr::Call { callee, args, .. } => {
            record_callee(callee, in_loop, scan);
            walk_expr(callee, in_loop, scan);
            for a in args {
                walk_expr(a, in_loop, scan);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            record_callee(callee, in_loop, scan);
            walk_expr(callee, in_loop, scan);
            walk_call_args(args, in_loop, scan);
        }

        // A closure introduces its own (unknown) invocation frequency: a loop
        // that merely *creates* a closure does not make the closure body hot.
        // Reset `in_loop` so calls inside the closure are only hot relative to
        // loops nested within the closure itself.
        Expr::Closure { body, .. } => walk_stmts(body, false, scan),

        // Assignment / update carriers.
        Expr::LocalSet(_, value) | Expr::GlobalSet(_, value) => walk_expr(value, in_loop, scan),

        // Arithmetic / logical / comparison trees.
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            walk_expr(left, in_loop, scan);
            walk_expr(right, in_loop, scan);
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::Await(operand)
        | Expr::Delete(operand)
        | Expr::StringCoerce(operand)
        | Expr::ObjectCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand) => walk_expr(operand, in_loop, scan),

        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            walk_expr(condition, in_loop, scan);
            walk_expr(then_expr, in_loop, scan);
            walk_expr(else_expr, in_loop, scan);
        }

        // Member / index access + writes.
        Expr::PropertyGet { object, .. } | Expr::PropertyUpdate { object, .. } => {
            walk_expr(object, in_loop, scan)
        }
        Expr::PropertySet { object, value, .. } => {
            walk_expr(object, in_loop, scan);
            walk_expr(value, in_loop, scan);
        }
        Expr::IndexGet { object, index } => {
            walk_expr(object, in_loop, scan);
            walk_expr(index, in_loop, scan);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            walk_expr(object, in_loop, scan);
            walk_expr(index, in_loop, scan);
            walk_expr(value, in_loop, scan);
        }
        Expr::IndexUpdate { object, index, .. } => {
            walk_expr(object, in_loop, scan);
            walk_expr(index, in_loop, scan);
        }

        // Method-call forms: not `FuncRef` callees, but their receiver/args can
        // hold hot calls or closures.
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                walk_expr(o, in_loop, scan);
            }
            for a in args {
                walk_expr(a, in_loop, scan);
            }
        }
        Expr::StaticMethodCall { args, .. } => {
            for a in args {
                walk_expr(a, in_loop, scan);
            }
        }

        // Aggregates.
        Expr::Array(elements) => {
            for el in elements {
                walk_expr(el, in_loop, scan);
            }
        }
        Expr::ArraySpread(elements) => {
            for el in elements {
                match el {
                    perry_hir::ArrayElement::Expr(e) | perry_hir::ArrayElement::Spread(e) => {
                        walk_expr(e, in_loop, scan)
                    }
                    perry_hir::ArrayElement::Hole => {}
                }
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                walk_expr(v, in_loop, scan);
            }
        }
        Expr::Sequence(es) => {
            for e in es {
                walk_expr(e, in_loop, scan);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                walk_expr(a, in_loop, scan);
            }
        }

        // Everything else (literals, refs, and the long tail of runtime
        // intrinsics) can't reach a hot `FuncRef` call in the patterns this
        // heuristic targets; not descending is a safe under-approximation.
        _ => {}
    }
}

/// Count every `new` emitted in every closure body, keyed by the closure's
/// FuncId. This is deliberately separate from [`walk_expr`]: that hot-callee
/// walker is an under-approximation, while the byte cap must be exhaustive.
/// `walk_expr_children` is the HIR's checked source of truth for expression
/// children, including parameter defaults; closure statement bodies are the
/// only boundary it intentionally leaves to consumers.
fn collect_closure_alloc_sites(hir: &Module) -> HashMap<u32, u32> {
    let mut sites = HashMap::new();
    count_alloc_sites_in_stmts(&hir.init, None, &mut sites);
    for f in &hir.functions {
        count_alloc_sites_in_stmts(&f.body, None, &mut sites);
    }
    for c in &hir.classes {
        if let Some(ctor) = &c.constructor {
            count_alloc_sites_in_stmts(&ctor.body, None, &mut sites);
        }
        for m in c.methods.iter().chain(c.static_methods.iter()) {
            count_alloc_sites_in_stmts(&m.body, None, &mut sites);
        }
        for (_, g) in &c.getters {
            count_alloc_sites_in_stmts(&g.body, None, &mut sites);
        }
        for (_, s) in &c.setters {
            count_alloc_sites_in_stmts(&s.body, None, &mut sites);
        }
        for cm in &c.computed_members {
            count_alloc_sites_in_expr(&cm.key_expr, None, &mut sites);
            count_alloc_sites_in_stmts(&cm.function.body, None, &mut sites);
        }
        for field in c.fields.iter().chain(c.static_fields.iter()) {
            if let Some(key) = &field.key_expr {
                count_alloc_sites_in_expr(key, None, &mut sites);
            }
            if let Some(init) = &field.init {
                count_alloc_sites_in_expr(init, None, &mut sites);
            }
        }
    }
    sites
}

fn count_alloc_sites_in_stmts(
    stmts: &[Stmt],
    closure_id: Option<u32>,
    sites: &mut HashMap<u32, u32>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                init: Some(expr), ..
            }
            | Stmt::Expr(expr)
            | Stmt::Throw(expr)
            | Stmt::Return(Some(expr)) => count_alloc_sites_in_expr(expr, closure_id, sites),
            Stmt::Let { init: None, .. } | Stmt::Return(None) => {}
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                count_alloc_sites_in_expr(condition, closure_id, sites);
                count_alloc_sites_in_stmts(then_branch, closure_id, sites);
                if let Some(else_branch) = else_branch {
                    count_alloc_sites_in_stmts(else_branch, closure_id, sites);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                count_alloc_sites_in_expr(condition, closure_id, sites);
                count_alloc_sites_in_stmts(body, closure_id, sites);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    count_alloc_sites_in_stmts(
                        std::slice::from_ref(init.as_ref()),
                        closure_id,
                        sites,
                    );
                }
                if let Some(condition) = condition {
                    count_alloc_sites_in_expr(condition, closure_id, sites);
                }
                if let Some(update) = update {
                    count_alloc_sites_in_expr(update, closure_id, sites);
                }
                count_alloc_sites_in_stmts(body, closure_id, sites);
            }
            Stmt::Labeled { body, .. } => {
                count_alloc_sites_in_stmts(std::slice::from_ref(body.as_ref()), closure_id, sites)
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                count_alloc_sites_in_stmts(body, closure_id, sites);
                if let Some(catch) = catch {
                    count_alloc_sites_in_stmts(&catch.body, closure_id, sites);
                }
                if let Some(finally) = finally {
                    count_alloc_sites_in_stmts(finally, closure_id, sites);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                count_alloc_sites_in_expr(discriminant, closure_id, sites);
                for case in cases {
                    if let Some(test) = &case.test {
                        count_alloc_sites_in_expr(test, closure_id, sites);
                    }
                    count_alloc_sites_in_stmts(&case.body, closure_id, sites);
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
}

fn count_alloc_sites_in_expr(expr: &Expr, closure_id: Option<u32>, sites: &mut HashMap<u32, u32>) {
    if let Expr::Closure {
        func_id,
        body,
        params: _,
        ..
    } = expr
    {
        sites.entry(*func_id).or_insert(0);
        // The canonical child walker visits this closure's param defaults.
        perry_hir::walker::walk_expr_children(expr, &mut |child| {
            count_alloc_sites_in_expr(child, Some(*func_id), sites)
        });
        count_alloc_sites_in_stmts(body, Some(*func_id), sites);
        return;
    }

    if matches!(expr, Expr::New { .. }) {
        if let Some(func_id) = closure_id {
            let count = sites.entry(func_id).or_insert(0);
            *count = count.saturating_add(1);
        }
    }
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        count_alloc_sites_in_expr(child, closure_id, sites)
    });
}
