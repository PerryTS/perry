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

/// #8872 follow-up: tiny allocation-bearing instance methods are allocation
/// kernels even when their callers live in another module and this module's
/// lexical loop scan cannot see them.  Keep the admission deliberately
/// bounded on both axes that contribute code size:
///
/// * at most two HIR statements per method, so this does not become a generic
///   "methods are hot" rule; and
/// * at most eight admitted `new` sites across the module, the same ~2.1 KiB
///   worst-case budget used for indirect closure calls above.
///
/// The motivating shape is a command-buffer method whose whole body is an
/// optional-argument prologue plus `commands.push({ ... })`.  Cross-module
/// callers make call-site hotness invisible here, but the method itself is a
/// stable, reusable allocation site.  The outlined allocator is semantically
/// identical, so declining modules over the budget is a safe under-inclusion.
const TINY_METHOD_MAX_STMTS: usize = 2;
const TINY_METHOD_ALLOC_SITE_BUDGET: u32 = 8;

/// The receiver-binding local `perry-transform`'s `field_push_local_bind`
/// pass introduces when it expands one `this.f.push(v)` statement into two
/// (`let __push_recv = this.f; push` — the write-back rides on the
/// `ArrayPush` node itself). For the tiny-method budget above that is still
/// the ONE statement the author wrote: the pass exists so the push takes the
/// inline append, and a command-buffer method that is exactly
/// `this.commands.push({ ... })` must not lose its allocation kernel to the
/// rewrite that made its push cheaper. Kept in sync by name with the pass
/// (`field_push_local_bind.rs`); the test below pins the shape.
const FIELD_PUSH_RECEIVER_NAME: &str = "__push_recv";

/// Statement count for the tiny-method rule: each field-push expansion
/// counts as the single statement it came from.
fn tiny_method_stmt_count(body: &[Stmt]) -> usize {
    let expansions = body
        .iter()
        .filter(|stmt| matches!(stmt, Stmt::Let { name, .. } if name == FIELD_PUSH_RECEIVER_NAME))
        .count();
    body.len().saturating_sub(expansions)
}

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
/// ## The four admission rules
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
/// 4. **Bounded tiny allocation methods** — instance methods with at most two
///    HIR statements, admitted only when their `new` sites total at most eight
///    across the module.  This covers cross-module allocation kernels whose
///    call-site loop is not visible in their defining module without turning
///    every method allocation into an inline site.
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
    // Rule 4: a tiny method that exists chiefly to construct and publish a
    // value is its own allocation kernel.  Count only allocations owned by the
    // method body; `count_alloc_sites_in_stmts` switches ownership at closure
    // boundaries, so a nested callback's `new` is still governed by rule 3.
    // Select all or none after counting to keep the result independent of
    // class/method traversal order.
    let mut tiny_method_sites: HashMap<u32, u32> = HashMap::new();
    for class in &hir.classes {
        for method in &class.methods {
            if tiny_method_stmt_count(&method.body) > TINY_METHOD_MAX_STMTS {
                continue;
            }
            // Count into a scratch map because the ownership-aware walker also
            // records nested closures.  Only transfer the sites still owned by
            // the method; closure-owned sites remain exclusively under rule 3.
            let mut owned_sites = HashMap::new();
            count_alloc_sites_in_stmts(&method.body, Some(method.id), &mut owned_sites);
            if let Some(sites) = owned_sites.get(&method.id).copied() {
                tiny_method_sites.insert(method.id, sites);
            }
        }
    }
    tiny_method_sites.retain(|_, sites| *sites != 0);
    let tiny_method_site_count = tiny_method_sites
        .values()
        .copied()
        .fold(0_u32, u32::saturating_add);
    if tiny_method_site_count > 0 && tiny_method_site_count <= TINY_METHOD_ALLOC_SITE_BUDGET {
        hot.extend(tiny_method_sites.into_keys());
    }
    hot
}

/// Directly self-recursive top-level functions that also contain at least one
/// `new` site in their own body. These are the functions where threading the
/// stable inline-arena pointer through self calls can remove a runtime lookup
/// at every recursive level.
pub fn collect_self_recursive_allocators(hir: &Module) -> HashSet<u32> {
    hir.functions
        .iter()
        .filter_map(|f| {
            let mut calls = HotCalleeScan::default();
            walk_stmts(&f.body, false, &mut calls);
            if !calls.call_counts.contains_key(&f.id) {
                return None;
            }

            let mut sites = HashMap::new();
            count_alloc_sites_in_stmts(&f.body, Some(f.id), &mut sites);
            (sites.get(&f.id).copied().unwrap_or(0) != 0).then_some(f.id)
        })
        .collect()
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

/// #8175: `FuncId`s of this module's top-level functions that participate in
/// direct recursion — a self-call, or membership in a cycle of `FuncRef`
/// call edges among `hir.functions`.
///
/// Consumed by the specialization plan to decide which specialized clones
/// get the `preserve_nonecc` calling convention: the convention's boundary
/// cost (a normal-CC caller saves ~20 CSRs once per entry) amortizes only
/// under a recursive tree, so a non-recursive clone must never pay it.
///
/// Approximation direction, both ways bounded: edges come from the same
/// `HotCalleeScan` walker the inline heuristics use, so a call inside a
/// nested closure counts as an edge from the enclosing function
/// (over-inclusion: at worst an extra boundary prologue at the clone's
/// non-recursive entries), while indirect calls and calls routed through
/// class methods are invisible (under-inclusion: a mutually-recursive pair
/// hiding behind an indirect hop keeps today's convention and forgoes the
/// win). Neither direction affects correctness — every caller of the clone
/// uses whatever convention the registry says, consistently.
pub(crate) fn collect_recursion_participants(hir: &Module) -> HashSet<u32> {
    let idx_of: HashMap<u32, usize> = hir
        .functions
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id, i))
        .collect();
    let n = hir.functions.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut participants: HashSet<u32> = HashSet::new();
    for (i, f) in hir.functions.iter().enumerate() {
        let mut scan = HotCalleeScan::default();
        walk_stmts(&f.body, false, &mut scan);
        if scan.call_counts.contains_key(&f.id) {
            participants.insert(f.id);
        }
        let mut callees: Vec<usize> = scan
            .call_counts
            .keys()
            .filter_map(|id| idx_of.get(id).copied())
            .collect();
        callees.sort_unstable();
        adj[i] = callees;
    }

    // Iterative Tarjan SCC — any component with more than one member is a
    // mutual-recursion cycle. Iterative because a deep static call chain is
    // user input, not something the compiler may overflow its stack on.
    const UNVISITED: u32 = u32::MAX;
    let mut index = vec![UNVISITED; n];
    let mut low = vec![0u32; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut next_index: u32 = 0;
    // (node, next child position in adj[node])
    let mut frames: Vec<(usize, usize)> = Vec::new();
    for start in 0..n {
        if index[start] != UNVISITED {
            continue;
        }
        frames.push((start, 0));
        index[start] = next_index;
        low[start] = next_index;
        next_index += 1;
        stack.push(start);
        on_stack[start] = true;
        while let Some(&mut (v, ref mut ci)) = frames.last_mut() {
            if let Some(&w) = adj[v].get(*ci) {
                *ci += 1;
                if index[w] == UNVISITED {
                    index[w] = next_index;
                    low[w] = next_index;
                    next_index += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    frames.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            frames.pop();
            if let Some(&(parent, _)) = frames.last() {
                low[parent] = low[parent].min(low[v]);
            }
            if low[v] == index[v] {
                // Root of an SCC: pop the component.
                let mut component: Vec<usize> = Vec::new();
                while let Some(w) = stack.pop() {
                    on_stack[w] = false;
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                if component.len() > 1 {
                    participants.extend(component.iter().map(|&w| hir.functions[w].id));
                }
            }
        }
    }
    participants
}

#[cfg(test)]
mod recursion_participant_tests {
    use super::*;
    use perry_hir::types::Type;
    use perry_hir::{Function, Param};

    fn func(id: u32, body: Vec<Stmt>) -> Function {
        Function {
            id,
            name: format!("f{id}"),
            type_params: Vec::new(),
            params: vec![Param {
                id: id * 10,
                name: "n".to_string(),
                ty: Type::Number,
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            }],
            return_type: Type::Number,
            body,
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }
    }

    fn call_stmt(fid: u32) -> Stmt {
        Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::FuncRef(fid)),
            args: vec![Expr::Integer(1)],
            type_args: Vec::new(),
            byte_offset: 0,
        }))
    }

    fn new_expr() -> Expr {
        Expr::New {
            class_name: "Command".to_string(),
            args: Vec::new(),
            cap_args_appended: 0,
            type_args: Vec::new(),
            byte_offset: 0,
        }
    }

    /// `this.commands.push({ ... })` after `field_push_local_bind` expanded it.
    fn expanded_field_push(recv_id: u32) -> Vec<Stmt> {
        vec![
            Stmt::Let {
                id: recv_id,
                name: FIELD_PUSH_RECEIVER_NAME.to_string(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::PropertyGet {
                    object: Box::new(Expr::This),
                    property: "commands".to_string(),
                    byte_offset: 0,
                }),
            },
            Stmt::Expr(Expr::ArrayPush {
                array_id: recv_id,
                value: Box::new(new_expr()),
                field_writeback: Some("commands".to_string()),
            }),
        ]
    }

    fn class_with_method(method: Function) -> perry_hir::Class {
        perry_hir::Class {
            id: 1,
            name: "CommandBuffer".to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: None,
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields: Vec::new(),
            constructor: None,
            methods: vec![method],
            getters: Vec::new(),
            setters: Vec::new(),
            static_accessor_names: Vec::new(),
            static_accessor_fn_ids: Vec::new(),
            computed_members: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            aliases: Vec::new(),
            is_nested: false,
            alloc_width_hint: 0,
            specialized_from: None,
        }
    }

    /// Rule 4 must see through `field_push_local_bind`'s expansion: a method
    /// that was `this.commands.push({ ... })` is still a tiny allocation
    /// kernel after the pass rewrote its push, while four genuinely separate
    /// statements still exceed the budget.
    #[test]
    fn tiny_method_rule_counts_a_field_push_expansion_as_one_statement() {
        let mut module = Module::new("buffer.ts");
        module
            .classes
            .push(class_with_method(func(11, expanded_field_push(101))));
        // Two copies back to back: two authored statements, not one.
        let mut two = expanded_field_push(201);
        two.extend(expanded_field_push(202));
        module.classes.push(class_with_method(func(12, two)));
        // The same two statements, but the local is an ordinary one: not an
        // expansion, so the method is two statements long.
        let mut plain = expanded_field_push(301);
        if let Stmt::Let { name, .. } = &mut plain[0] {
            *name = "old".to_string();
        }
        module.classes.push(class_with_method(func(13, plain)));

        assert_eq!(
            tiny_method_stmt_count(&module.classes[0].methods[0].body),
            1
        );
        assert_eq!(
            tiny_method_stmt_count(&module.classes[1].methods[0].body),
            2
        );
        assert_eq!(
            tiny_method_stmt_count(&module.classes[2].methods[0].body),
            2
        );
        let hot = collect_alloc_hot_functions(&module);
        assert!(
            hot.contains(&11),
            "the expanded field push is still a tiny kernel: {hot:?}"
        );
    }

    #[test]
    fn self_call_cycle_pair_and_acyclic_chain_are_classified() {
        let mut module = Module::new("recursion.ts");
        // 1 calls itself; 2 <-> 3 mutual; 4 -> 5 -> 6 straight chain;
        // 7 -> 1 calls INTO a cycle without being on one.
        module.functions.push(func(1, vec![call_stmt(1)]));
        module.functions.push(func(2, vec![call_stmt(3)]));
        module.functions.push(func(3, vec![call_stmt(2)]));
        module.functions.push(func(4, vec![call_stmt(5)]));
        module.functions.push(func(5, vec![call_stmt(6)]));
        module
            .functions
            .push(func(6, vec![Stmt::Return(Some(Expr::Integer(0)))]));
        module.functions.push(func(7, vec![call_stmt(1)]));

        let got = collect_recursion_participants(&module);
        let mut got: Vec<u32> = got.into_iter().collect();
        got.sort_unstable();
        assert_eq!(
            got,
            vec![1, 2, 3],
            "participants are exactly the self-loop and the 2-cycle; \
             a caller INTO a cycle and an acyclic chain stay out"
        );
    }

    #[test]
    fn a_long_static_call_chain_does_not_overflow_the_walker() {
        // The SCC walk is iterative on purpose: chain depth is user input.
        let mut module = Module::new("recursion.ts");
        const N: u32 = 20_000;
        for id in 1..=N {
            let body = if id < N {
                vec![call_stmt(id + 1)]
            } else {
                vec![call_stmt(1)] // close one giant cycle
            };
            module.functions.push(func(id, body));
        }
        let got = collect_recursion_participants(&module);
        assert_eq!(got.len() as u32, N, "the whole ring is one SCC");
    }
}
