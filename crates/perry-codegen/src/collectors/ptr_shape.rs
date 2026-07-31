//! Representation-selection Phase 3b (RFC `docs/representation-selection-rfc.md`
//! §4 Object row, §5.5-§5.7): shape-proven object locals (`Ptr<Shape>`).
//!
//! ## What this proves
//!
//! A function-local `let o = new C(...)` qualifies as a **shape-proven pointer
//! local** when static analysis proves that the object's shape (class identity,
//! key set, field layout, descriptor-free-ness, method table) cannot change for
//! the local's entire lifetime. Every field access on such a local then lowers
//! to the bare fixed-offset form — `load i64` slot, `and` POINTER_MASK, `gep
//! +header`, `gep` index, `load` — with **no per-access guard diamond** (no
//! volatile gate load, no 7-header-load shape check, no
//! `js_typed_feedback_class_field_*_guard` fallback arm, no `phi`), and method
//! calls on it lower to a **direct call with no shape guard**.
//!
//! ## Why it is sound without the runtime invalidators
//!
//! Today's guarded fast path is per-SITE and runtime-checked: the process-global
//! sticky gate (`PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED`), the per-object
//! descriptor flag, and prototype-guard invalidation exist because a receiver
//! at an arbitrary site can be *any* object. A `Ptr<Shape>` local is instead
//! proven by **provenance + containment**:
//!
//! 1. **Provenance**: the local is initialized by exactly one `Stmt::Let` whose
//!    init is `new C(...)` — its dynamic class is *exactly* `C` (Perry class
//!    constructors cannot return an override object). Anon-shape literals
//!    (`{k: v}` closed shapes) and `{}` builder sites also lower to
//!    `Expr::New { class_name: "__AnonShape_…" }`, so records qualify through
//!    the same test. Since #7034 §4 a direct call to a module function
//!    carrying a **return-shape fact** is provenance of the same strength —
//!    that fact certifies the callee hands back a freshly allocated, unaliased
//!    `C` on every return path (`collectors/ptr_shape_returns.rs`).
//! 2. **Containment**: every use of the local is a declared-chain field
//!    read/write/update or a vetted method call. Any other use — reassignment,
//!    closure capture, call argument, array/object element, throw,
//!    `delete`, freeze/seal, aliasing — disqualifies. The object is therefore
//!    unreachable from anywhere except this local, so no §5.2 barrier
//!    (defineProperty / delete / setPrototypeOf / Proxy / mutating Reflect)
//!    can reach it *through an alias*.
//!
//!    **Exception — the return position (#7034 §4).** `return <the local>`
//!    does NOT disqualify. Containment exists to bound the object's aliases
//!    *while this function still reads it*, and a `return` is a terminator:
//!    every use of the local in this body either precedes it on that path or
//!    is unreachable from it, the sole exception being a `finally` block —
//!    which still runs before the caller resumes, and whose uses this same
//!    walk checks anyway. The caller cannot have touched the object yet, so
//!    no shape transition can have happened at any access this pass licenses.
//!    Returns nested inside a CLOSURE body are not exempt: that value escapes
//!    at an unbounded later time (`UseWalk::in_closure`).
//! 3. **`this`-flow containment**: the constructor chain, chain field
//!    initializers, and every method called on the local are walked with a
//!    strict `this`-usage discipline (field access on `this`, vetted
//!    `this.m()` / `super` chains only). Any leak of `this` as a value —
//!    which would create an alias the escape walk cannot see — disqualifies.
//! 4. **Dispatch stability**: method calls additionally require
//!    [`ModuleDispatchFacts::prototype_is_stable`] (the same module-level scan
//!    shipped for scalar-replacement method summaries, #5872) and no
//!    own-property write that could shadow the method (subsumed by rule 2:
//!    writes to non-declared-field names disqualify).
//! 5. **Module-wide unbounded-barrier policy (first increment)**: if the
//!    module contains *any* `Object.defineProperty`-family site, `delete`,
//!    `setPrototypeOf`/`__proto__` write, `Proxy` construction, or mutating
//!    `Reflect.*` call — regardless of target — ALL `Ptr<Shape>` promotion in
//!    the module is disabled (`ModuleDispatchFacts::shape_barrier_sites`).
//!    Rules 1-4 already bound every path to the object, so this module-wide
//!    kill is belt-and-braces against analysis blind spots; it is the
//!    conservative first-increment rule from the RFC §5.2 discussion and
//!    still covers the common barrier-free module. (`eval` needs no kill:
//!    Perry never executes a runtime code string — see
//!    `perry-hir/src/eval_classifier.rs`.)
//!
//! ## Numeric-proven fields
//!
//! For the READ side to keep today's `JsNumber`/`NativeRep::F64` semantics on a
//! bare `load double`, the analysis additionally proves per raw-f64-declared
//! field that **every reachable store** (constructor, chain field initializers,
//! method bodies, and in-function stores — an exhaustive set, by rule 2) is
//! number-producing by construction. Constructor/method parameter stores are
//! resolved through the actual argument expressions at the provenance `new` /
//! call sites. Fields that fail keep the bare load but surface as generic
//! `JsValue` (bit-identical — a NaN-boxed number IS its own double bits).
//! Store-side raw-slot discipline (plain-finite check + boxed-setter side
//! exit) is emitted at the access site regardless, so a NaN/Inf/non-number
//! store can never corrupt a scalar-masked slot the GC does not scan.
//!
//! ## GC contract (tagged-at-rest)
//!
//! The local's storage is untouched: the existing NaN-boxed slot, registered
//! as a rewritable root via `js_shadow_slot_bind` (the GC marks and rewrites
//! **through the bound alloca**, `gc/roots/shadow_stack.rs`). The raw pointer
//! is region-local SSA only: every access re-derives it from the slot, and
//! because the slot address escapes to the shadow-stack registry, LLVM cannot
//! CSE the reload across a safepoint — rebase-after-safepoint (RFC §5.6) falls
//! out of alias analysis. Raw pointers are never stored at rest.
//!
//! Gated by `PERRY_PTR_SHAPE_LOCALS` (default on; `0`/`off`/`false` disables —
//! keyed into the object cache). `PERRY_REPSEL_DEBUG=1` prints one line per
//! proven local at compile time.

use std::collections::{HashMap, HashSet};

use perry_hir::{Class, Expr, Stmt};

use super::ptr_shape_report as report;
use super::ptr_shape_report::ShapeDenial;
use super::ModuleDispatchFacts;
use crate::opt_report;

/// `PERRY_PTR_SHAPE_LOCALS` gate. Enabled by default; `=0`/`off`/`false`
/// disables shape-proven pointer-local selection (every access keeps today's
/// guarded lowering). Keyed into the object cache (`object_cache.rs`).
pub fn ptr_shape_locals_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_PTR_SHAPE_LOCALS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

fn repsel_debug_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| std::env::var("PERRY_REPSEL_DEBUG").as_deref() == Ok("1"))
}

/// A function-local proven to hold exactly one object of `class_name` for its
/// entire lifetime, with a statically-immutable shape. See the module doc for
/// the full proof obligations.
#[derive(Debug, Clone)]
pub struct PtrShapeLocal {
    /// The provenance class — the exact dynamic class of the object.
    pub class_name: String,
    /// Raw-f64-declared chain fields whose every reachable store is proven
    /// number-producing: bare loads may claim `JsNumber`/`F64`. Other fields'
    /// bare loads surface as generic `JsValue` (bit-identical).
    pub numeric_fields: HashSet<String>,
}

/// Whether an expression node is a §5.2 shape barrier for the module-wide
/// first-increment kill rule. Targets are NOT inspected — any occurrence
/// disables all `Ptr<Shape>` promotion in the module.
pub(crate) fn expr_is_shape_barrier(expr: &Expr) -> bool {
    match expr {
        Expr::ObjectDefineProperty(..)
        | Expr::ObjectDefineProperties(..)
        | Expr::ReflectDefineProperty { .. }
        | Expr::ObjectSetPrototypeOf(..)
        | Expr::ReflectSetPrototypeOf { .. }
        | Expr::ReflectSet { .. }
        | Expr::ReflectDelete { .. }
        | Expr::ReflectPreventExtensions(..)
        | Expr::Delete(..)
        | Expr::ProxyNew { .. } => true,
        // `__proto__` writes mutate the prototype chain of an arbitrary
        // object. (Reads and `<Class>.prototype` naming are handled by the
        // dispatch-stability facts; only writes are shape barriers.)
        Expr::PropertySet { property, .. } | Expr::PropertyUpdate { property, .. } => {
            property == "__proto__"
        }
        Expr::PutValueSet { key, .. } => {
            matches!(key.as_ref(), Expr::String(k) if k == "__proto__")
        }
        Expr::IndexSet { index, .. } => {
            matches!(index.as_ref(), Expr::String(k) if k == "__proto__")
        }
        _ => false,
    }
}

/// Compile-time visibility: one stderr line per shape-proven local, plus a
/// process-wide running count. Only under `PERRY_REPSEL_DEBUG=1`.
///
/// Also feeds the `--opt-report` win column (#6952) — the two mechanisms
/// share this one call site so a future proof cannot appear in one and not
/// the other.
fn note_ptr_shape_local(
    id: u32,
    fact: &PtrShapeLocal,
    names: &HashMap<u32, String>,
    depths: &HashMap<u32, u32>,
) {
    // #7034 §4: `report::suppressed()` is set while the return-shape module
    // pre-pass re-runs this proof speculatively — see `SuppressScope`.
    if report::suppressed() {
        return;
    }
    if opt_report::enabled() {
        let fallback = format!("<local {id}>");
        let name = names.get(&id).map(String::as_str).unwrap_or(&fallback);
        opt_report::select(
            opt_report::Position::Local,
            name,
            Some(id),
            opt_report::Analysis::PtrShape,
            "Ptr<Shape>",
            depths.get(&id).copied().unwrap_or(0),
            Some(format!(
                "class {} ({} numeric field(s) proven)",
                fact.class_name,
                fact.numeric_fields.len()
            )),
        );
    }
    if !repsel_debug_enabled() {
        return;
    }
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let n = COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!(
        "repsel: ptr-shape local id {id} class '{}' numeric_fields {:?} (total {n})",
        fact.class_name, fact.numeric_fields
    );
}

/// Record every candidate the collector will not even reach, on an
/// early-bail path (env gate off, or the rule-5 module-wide barrier).
///
/// Runs only under `--opt-report`; it re-derives the candidate seeds purely
/// to name them, and returns nothing the collector consumes — the bail-out
/// itself is unchanged.
fn report_early_bail(
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    denial: ShapeDenial,
) {
    if !opt_report::enabled() {
        return;
    }
    let names = report::local_names(stmts);
    let depths = report::loop_depths(stmts);
    let seeds = report::candidate_seeds(stmts, boxed_vars, module_globals);
    for (id, class_name) in &seeds {
        report::deny_local(*id, &names, &depths, Some(class_name), denial);
    }
    for site in report::unbound_new_sites(stmts) {
        report::deny_alloc_site(&site);
    }
}

/// Entry point: collect the shape-proven pointer locals of one lowered region.
///
/// `not_bigint_locals` feeds the numeric-field proof (a `Sub`/`Div`/bitwise
/// over provably-non-BigInt operands is a Number by spec).
pub(crate) fn collect_shape_proven_ptr_locals(
    stmts: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    classes: &HashMap<String, &Class>,
    module_dispatch: &ModuleDispatchFacts,
    not_bigint_locals: &HashSet<u32>,
) -> HashMap<u32, PtrShapeLocal> {
    if !ptr_shape_locals_enabled() {
        report_early_bail(stmts, boxed_vars, module_globals, report::GATE_DISABLED);
        return HashMap::new();
    }
    if module_dispatch.has_shape_barrier_sites() {
        report_early_bail(stmts, boxed_vars, module_globals, report::MODULE_BARRIER);
        return HashMap::new();
    }
    // `--opt-report` (#6952): binding names and loop depths for the values
    // this pass is about to accept or deny. Both walks are skipped entirely
    // when the report is off.
    let (names, depths) = if opt_report::enabled() {
        (report::local_names(stmts), report::loop_depths(stmts))
    } else {
        (HashMap::new(), HashMap::new())
    };
    if opt_report::enabled() {
        // Allocations that are never bound to a local — rule 1 can never see
        // them, and on real code they are the majority (#7034 §4).
        for site in report::unbound_new_sites(stmts) {
            report::deny_alloc_site(&site);
        }
    }
    // Pass 1: `Stmt::Let { init: New }` candidates, same seed as scalar
    // replacement (excludes boxed and module-global locals — which also
    // excludes async/generator bodies, whose locals are boxed by the
    // async-to-generator transform).
    let mut candidates: HashMap<u32, String> = HashMap::new();
    super::find_new_candidates(stmts, boxed_vars, module_globals, &mut candidates);
    // #7034 §4: `const r = producer(...)` where `producer` carries a
    // return-shape fact is provenance of `new`-strength (module doc, rule 1).
    let return_seeded = super::ptr_shape_returns::find_return_shape_candidates(
        stmts,
        boxed_vars,
        module_globals,
        module_dispatch,
        &mut candidates,
    );
    if candidates.is_empty() {
        return HashMap::new();
    }
    // Class-level admission BEFORE the use walk so the walk's chain-field
    // membership tests are meaningful.
    candidates.retain(|id, class_name| {
        match report::admission_cause(classes, class_name, &chain_classes(classes, class_name)) {
            None => true,
            Some(cause) => {
                report::deny_local(*id, &names, &depths, Some(class_name), cause);
                false
            }
        }
    });
    if candidates.is_empty() {
        return HashMap::new();
    }

    // Alias pre-pass: `const alias = candidate` (the exact-receiver inliner
    // materializes compound-assign receivers this way — `__cmpd_base_N`).
    // A non-mutable Let whose init is a bare LocalGet of a candidate (or of
    // another alias) tracks the SAME object; its uses follow the same rules
    // and attribute to the root. Mutable, boxed, or module-global aliases
    // stay untracked — a bare reference to the candidate through them then
    // disqualifies via the use walk, which is the sound default.
    let mut alias_edges: Vec<(u32, u32)> = Vec::new();
    collect_alias_edges(stmts, &mut alias_edges);
    let mut roots: HashMap<u32, u32> = candidates.keys().map(|id| (*id, *id)).collect();
    loop {
        let mut changed = false;
        for (alias, src) in &alias_edges {
            if candidates.contains_key(alias)
                || boxed_vars.contains(alias)
                || module_globals.contains_key(alias)
                || roots.contains_key(alias)
            {
                continue;
            }
            if let Some(&root) = roots.get(src) {
                roots.insert(*alias, root);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Pass 2: strict use walk.
    let mut walk = UseWalk {
        candidates: &candidates,
        roots: &roots,
        classes,
        disqualified: HashSet::new(),
        let_counts: HashMap::new(),
        field_stores: HashMap::new(),
        method_calls: HashMap::new(),
        new_args: HashMap::new(),
        const_local_inits: HashMap::new(),
        disq_reasons: HashMap::new(),
        escape_ctx: report::ESC_BARE_REFERENCE,
        return_seeded: &return_seeded,
        in_closure: false,
    };
    walk.walk_stmts(stmts);
    let UseWalk {
        disqualified,
        let_counts,
        field_stores,
        method_calls,
        new_args,
        const_local_inits,
        disq_reasons,
        ..
    } = walk;
    let mut out = HashMap::new();
    // `--opt-report`: one closure so every `continue` below has a matching
    // one-line recording. Behaviour is unchanged — `deny` is a no-op when
    // the report is off.
    let deny = |id: &u32, class_name: &String, why: ShapeDenial| {
        report::deny_local(*id, &names, &depths, Some(class_name), why);
    };
    'cand: for (id, class_name) in &candidates {
        if disqualified.contains(id) {
            deny(
                id,
                class_name,
                disq_reasons
                    .get(id)
                    .copied()
                    .unwrap_or(report::ESC_BARE_REFERENCE),
            );
            continue;
        }
        if let_counts.get(id).copied().unwrap_or(0) != 1 {
            deny(id, class_name, report::MULTIPLE_LET);
            continue;
        }
        // Every alias of this root must itself be single-Let (a re-declared
        // alias id would leave a second binding the proof does not cover).
        if roots
            .iter()
            .any(|(m, r)| r == id && m != id && let_counts.get(m).copied().unwrap_or(0) != 1)
        {
            deny(id, class_name, report::ALIAS_NOT_SINGLE_LET);
            continue;
        }
        let chain = chain_classes(classes, class_name);
        let fields = chain_field_names(&chain);
        let methods = chain_method_map(&chain);
        // Constructor chain + field initializers must not leak `this`. All
        // methods called on the local must be `this`-flow safe and the module
        // must prove the method table stable.
        let mut analysis = ThisFlowAnalysis {
            chain: &chain,
            fields: &fields,
            methods: &methods,
            visited: HashSet::new(),
            store_records: Vec::new(),
            super_call_args: HashMap::new(),
            internally_invoked: HashSet::new(),
            allow_this_in_store_values: false,
        };
        if !analysis.ctor_chain_safe() {
            deny(id, class_name, report::THIS_ESCAPE);
            continue;
        }
        let called = method_calls.get(id);
        if let Some(called) = called {
            if !module_dispatch.prototype_is_stable(classes, class_name) {
                deny(id, class_name, report::UNSTABLE_PROTOTYPE);
                continue;
            }
            for m in called.keys() {
                if fields.contains(m.as_str()) {
                    // A name that is both a field and a method is ambiguous
                    // under own-property shadowing — bail.
                    deny(id, class_name, report::FIELD_METHOD_AMBIGUITY);
                    continue 'cand;
                }
                let Some((owner, func)) = methods.get(m.as_str()) else {
                    deny(id, class_name, report::ESC_UNRESOLVED_METHOD);
                    continue 'cand;
                };
                if !analysis.method_safe(owner, func) {
                    deny(id, class_name, report::METHOD_THIS_ESCAPE);
                    continue 'cand;
                }
            }
        }
        let store_records = std::mem::take(&mut analysis.store_records);
        let super_call_args = std::mem::take(&mut analysis.super_call_args);
        let internally_invoked = std::mem::take(&mut analysis.internally_invoked);
        let members: HashSet<u32> = roots
            .iter()
            .filter(|(_, r)| *r == id)
            .map(|(m, _)| *m)
            .collect();
        // #7034 §4: a return-shape-seeded candidate NEVER claims numeric
        // fields. The numeric proof is an EXHAUSTIVE-reachable-store proof,
        // and the producer's own stores (`acc.weight = …` inside the callee)
        // are not in this region at all — claiming `JsNumber` off the
        // constructor's stores alone would let a guard-free `load double` in
        // a number context read a slot the producer had put a string in. The
        // shape proof by itself still retires the whole guard diamond; this
        // is the same stand-down `collectors/proven_this.rs` makes, for the
        // same reason.
        let numeric_fields = if return_seeded.contains(id) {
            HashSet::new()
        } else {
            prove_numeric_fields(
                &chain,
                &members,
                &store_records,
                field_stores.get(id).map(Vec::as_slice).unwrap_or(&[]),
                new_args.get(id).copied().unwrap_or(&[]),
                called,
                &super_call_args,
                &internally_invoked,
                not_bigint_locals,
                &const_local_inits,
            )
        };
        let fact = PtrShapeLocal {
            class_name: class_name.clone(),
            numeric_fields,
        };
        note_ptr_shape_local(*id, &fact, &names, &depths);
        // Aliases carry the same fact: they hold the same object, their slots
        // are equally shadow-bound, and access sites key on the local they
        // actually reference.
        for (member, root) in &roots {
            if root == id && member != id {
                out.insert(*member, fact.clone());
            }
        }
        out.insert(*id, fact);
    }
    out
}

/// Collect `Let { mutable: false, init: Some(LocalGet(src)) }` edges — the
/// alias shape the exact-receiver inliner emits for compound assigns.
fn collect_alias_edges(stmts: &[Stmt], out: &mut Vec<(u32, u32)>) {
    for s in stmts {
        match s {
            Stmt::Let {
                id,
                mutable: false,
                init: Some(Expr::LocalGet(src)),
                ..
            } => out.push((*id, *src)),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_alias_edges(then_branch, out);
                if let Some(eb) = else_branch {
                    collect_alias_edges(eb, out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_alias_edges(body, out);
            }
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    collect_alias_edges(std::slice::from_ref(init.as_ref()), out);
                }
                collect_alias_edges(body, out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_alias_edges(body, out);
                if let Some(c) = catch {
                    collect_alias_edges(&c.body, out);
                }
                if let Some(f) = finally {
                    collect_alias_edges(f, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_alias_edges(&case.body, out);
                }
            }
            Stmt::Labeled { body, .. } => {
                collect_alias_edges(std::slice::from_ref(body.as_ref()), out);
            }
            _ => {}
        }
    }
}

// ── Class-level admission ──────────────────────────────────────────────────

/// The chain (self first) when every link is a modeled, accessor-free,
/// computed-free, statically-extended user class; `None`-equivalent (empty)
/// otherwise.
pub(super) fn chain_classes<'a>(
    classes: &HashMap<String, &'a Class>,
    class_name: &str,
) -> Vec<&'a Class> {
    let mut out = Vec::new();
    let mut current = Some(class_name.to_string());
    let mut seen = HashSet::new();
    while let Some(name) = current {
        if !seen.insert(name.clone()) || out.len() > 64 {
            return Vec::new();
        }
        let Some(class) = classes.get(&name).copied() else {
            return Vec::new();
        };
        out.push(class);
        current = class.extends_name.clone();
    }
    out
}

/// Class-level admission. Delegates to
/// [`super::ptr_shape_report::admission_cause`], which enumerates the same
/// disqualifiers and additionally *names* the first one that fired so
/// `--opt-report` can report it. Keeping one implementation means the gate
/// and the explanation can never disagree.
///
/// The disqualifiers are: an empty/unresolvable chain, a getter or setter, a
/// computed member or computed field key, a dynamic or lexically-shadowed
/// `extends`, an `extends` ClassId with no resolvable `extends_name`, a
/// native base, and (from the shipped scalar-replacement rejections) a
/// built-in Error base or an unmodeled base — the latter two install fields
/// or stamp their method surface as own properties at run time.
pub(super) fn chain_admissible(classes: &HashMap<String, &Class>, class_name: &str) -> bool {
    let chain = chain_classes(classes, class_name);
    super::ptr_shape_report::admission_cause(classes, class_name, &chain).is_none()
}

pub(super) fn chain_field_names(chain: &[&Class]) -> HashSet<String> {
    let mut out = HashSet::new();
    for class in chain {
        out.extend(class.fields.iter().map(|f| f.name.clone()));
    }
    out
}

/// name -> (owning class name, method function), first (most-derived) wins —
/// matching JS prototype-chain resolution for an exact-class instance.
pub(super) fn chain_method_map<'a>(
    chain: &[&'a Class],
) -> HashMap<String, (String, &'a perry_hir::Function)> {
    let mut out: HashMap<String, (String, &perry_hir::Function)> = HashMap::new();
    for class in chain {
        for method in &class.methods {
            out.entry(method.name.clone())
                .or_insert_with(|| (class.name.clone(), method));
        }
    }
    out
}

// ── Pass 2: strict use walk ────────────────────────────────────────────────

/// A recorded store into a candidate's field, with enough context to resolve
/// parameter-mediated values later.
enum StoreValue<'a> {
    /// Value expression in function scope (a direct `o.f = expr` store).
    Direct(&'a Expr),
    /// `++`/`--` — always numeric when the old value is numeric (recorded as
    /// unconditionally numeric: ToNumeric of a proven-number field is that
    /// number; non-proven fields are not claimed numeric anyway).
    Update,
}

struct UseWalk<'a> {
    candidates: &'a HashMap<u32, String>,
    /// Tracked member id (candidate or const alias) -> root candidate id.
    roots: &'a HashMap<u32, u32>,
    classes: &'a HashMap<String, &'a Class>,
    disqualified: HashSet<u32>,
    let_counts: HashMap<u32, u32>,
    /// root candidate -> (field name, store value) for in-function stores.
    field_stores: HashMap<u32, Vec<(String, StoreValue<'a>)>>,
    /// root candidate -> method name -> per-call-site argument lists.
    method_calls: HashMap<u32, HashMap<String, Vec<&'a [Expr]>>>,
    /// root candidate -> the provenance `new C(...)` argument list.
    new_args: HashMap<u32, &'a [Expr]>,
    /// Non-tracked `const` locals' init expressions (single-Let only; a
    /// re-declared id is poisoned to `None`). Lets the numeric-field proof
    /// chase one level through `const v = i * 0.5`-style temps.
    const_local_inits: HashMap<u32, Option<&'a Expr>>,
    /// `--opt-report` (#6952): root candidate -> the FIRST use that
    /// disqualified it. Purely observational — `disqualified` is the set the
    /// proof consults; this only records why.
    disq_reasons: HashMap<u32, ShapeDenial>,
    /// The escape kind a bare `LocalGet` in the current position implies.
    /// Parent arms narrow it (`return`, call argument, array element, …) so
    /// the report can say *how* the object escaped, not just that it did.
    escape_ctx: ShapeDenial,
    /// #7034 §4: candidates whose provenance is a return-shape-carrying CALL
    /// rather than a `new`. Their `Let` init is an `Expr::Call`, which rule 1
    /// would otherwise reject as `LET_INIT_NOT_NEW`.
    return_seeded: &'a HashSet<u32>,
    /// #7034 §4: are we inside a closure body? A `return <candidate>` there
    /// escapes at an unbounded later time, so the return exemption (module
    /// doc, rule 2) does NOT apply — only the enclosing function's own
    /// returns are terminators for this local's lifetime.
    in_closure: bool,
}

impl<'a> UseWalk<'a> {
    /// Root candidate for a tracked member id (candidate or alias).
    fn tracked_root(&self, id: u32) -> Option<u32> {
        self.roots.get(&id).copied()
    }

    fn disq(&mut self, id: u32, why: ShapeDenial) {
        if let Some(root) = self.tracked_root(id) {
            self.disqualified.insert(root);
            self.note_reason(root, why);
        }
    }

    /// Disqualify a root that has already been resolved.
    fn disq_root(&mut self, root: u32, why: ShapeDenial) {
        self.disqualified.insert(root);
        self.note_reason(root, why);
    }

    /// First reason wins: it is the use the developer will find first, and
    /// later uses are usually consequences of the same escape.
    fn note_reason(&mut self, root: u32, why: ShapeDenial) {
        if !opt_report::enabled() {
            return;
        }
        self.disq_reasons.entry(root).or_insert(why);
    }

    /// Run `f` with the bare-reference escape kind narrowed to `why`.
    fn with_ctx(&mut self, why: ShapeDenial, f: impl FnOnce(&mut Self)) {
        let previous = self.escape_ctx;
        self.escape_ctx = why;
        f(self);
        self.escape_ctx = previous;
    }

    fn candidate_chain_has_field(&self, root: u32, property: &str) -> bool {
        let Some(class_name) = self.candidates.get(&root) else {
            return false;
        };
        let chain = chain_classes(self.classes, class_name);
        chain
            .iter()
            .any(|c| c.fields.iter().any(|f| f.name == property))
    }

    fn walk_stmts(&mut self, stmts: &'a [Stmt]) {
        for s in stmts {
            self.walk_stmt(s);
        }
    }

    fn walk_stmt(&mut self, s: &'a Stmt) {
        match s {
            Stmt::Let { id, init, .. } => {
                if self.candidates.contains_key(id) {
                    *self.let_counts.entry(*id).or_insert(0) += 1;
                    if let Some(Expr::New { args, .. }) = init.as_ref() {
                        self.new_args.insert(*id, args.as_slice());
                        for a in args {
                            self.walk_expr(a);
                        }
                        return;
                    }
                    // #7034 §4: a return-shape-seeded candidate's provenance
                    // is the CALL. It records no `new_args` — the constructor
                    // ran in the callee, so the numeric-field proof stands
                    // down for these candidates entirely (see the `'cand`
                    // loop). The argument expressions are ordinary values;
                    // walk them so OTHER candidates passed there still escape.
                    if self.return_seeded.contains(id) {
                        if let Some(Expr::Call { args, .. }) = init.as_ref() {
                            for a in args {
                                self.with_ctx(report::ESC_CALL_ARGUMENT, |w| w.walk_expr(a));
                            }
                            return;
                        }
                    }
                    // A candidate whose Let init is not the New (var-redecl
                    // seed) is not provenance-stable.
                    self.disq(*id, report::LET_INIT_NOT_NEW);
                } else if !self.roots.contains_key(id) {
                    // Plain local: remember single-Let const inits for the
                    // numeric proof; poison re-declared ids.
                    if let Stmt::Let {
                        mutable: false,
                        init: Some(init),
                        ..
                    } = s
                    {
                        match self.const_local_inits.entry(*id) {
                            std::collections::hash_map::Entry::Vacant(e) => {
                                e.insert(Some(init));
                            }
                            std::collections::hash_map::Entry::Occupied(mut e) => {
                                e.insert(None);
                            }
                        }
                    } else {
                        self.const_local_inits.insert(*id, None);
                    }
                    if let Some(e) = init {
                        self.walk_expr(e);
                    }
                    return;
                } else if let Some(root) = self.tracked_root(*id) {
                    // Alias binding: `const alias = <member of same root>` is
                    // the tracked edge itself — count it, don't treat the
                    // init's LocalGet as an escape. Any other init shape for
                    // an alias id disqualifies the root (re-declared alias).
                    *self.let_counts.entry(*id).or_insert(0) += 1;
                    match init.as_ref() {
                        Some(Expr::LocalGet(src)) if self.tracked_root(*src) == Some(root) => {
                            return;
                        }
                        _ => self.disq_root(root, report::ALIAS_NOT_SINGLE_LET),
                    }
                }
                if let Some(e) = init {
                    self.walk_expr(e);
                }
            }
            Stmt::Expr(e) => self.walk_expr(e),
            Stmt::Throw(e) => self.with_ctx(report::ESC_THROWN, |w| w.walk_expr(e)),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    // #7034 §4: `return <tracked local>` is exempt — see the
                    // module doc, rule 2. Only the bare form: `return {a: o}`
                    // or `return f(o)` embeds the object in a value whose
                    // other references this walk has not bounded, and a
                    // return inside a closure body is not a terminator for
                    // the enclosing function's local.
                    if !self.in_closure {
                        if let Expr::LocalGet(id) = e {
                            if self.tracked_root(*id).is_some() {
                                return;
                            }
                        }
                    }
                    self.with_ctx(report::ESC_RETURN, |w| w.walk_expr(e));
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(condition);
                self.walk_stmts(then_branch);
                if let Some(eb) = else_branch {
                    self.walk_stmts(eb);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.walk_expr(condition);
                self.walk_stmts(body);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.walk_stmt(init.as_ref());
                }
                if let Some(c) = condition {
                    self.walk_expr(c);
                }
                if let Some(u) = update {
                    self.walk_expr(u);
                }
                self.walk_stmts(body);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.walk_stmts(body);
                if let Some(c) = catch {
                    self.walk_stmts(&c.body);
                }
                if let Some(f) = finally {
                    self.walk_stmts(f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.walk_expr(discriminant);
                for case in cases {
                    if let Some(t) = &case.test {
                        self.walk_expr(t);
                    }
                    self.walk_stmts(&case.body);
                }
            }
            Stmt::Labeled { body, .. } => self.walk_stmt(body.as_ref()),
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_) => {}
        }
    }

    fn walk_expr(&mut self, e: &'a Expr) {
        match e {
            // Safe: declared-chain field read on a tracked member.
            Expr::PropertyGet {
                object, property, ..
            } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    if let Some(root) = self.tracked_root(*id) {
                        if !self.candidate_chain_has_field(root, property) {
                            self.disq_root(root, report::ESC_UNDECLARED_PROPERTY);
                        }
                        return;
                    }
                }
                self.walk_expr(object);
            }
            // Safe: declared-chain field write on a tracked member (value must
            // not reference the same object — that would embed an untracked
            // alias reachable through a field read).
            Expr::PropertySet {
                object,
                property,
                value,
            } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    if let Some(root) = self.tracked_root(*id) {
                        if !self.candidate_chain_has_field(root, property) {
                            self.disq_root(root, report::ESC_UNDECLARED_PROPERTY);
                        } else {
                            self.field_stores
                                .entry(root)
                                .or_default()
                                .push((property.clone(), StoreValue::Direct(value)));
                        }
                        // The value walk is position-aware: a field read of the
                        // same object is safe; a BARE reference to it (e.g.
                        // `o.self = o`) hits the LocalGet arm and escapes.
                        self.with_ctx(report::ESC_ELEMENT, |w| w.walk_expr(value));
                        return;
                    }
                }
                self.walk_expr(object);
                self.walk_expr(value);
            }
            Expr::PropertyUpdate {
                object, property, ..
            } => {
                if let Expr::LocalGet(id) = object.as_ref() {
                    if let Some(root) = self.tracked_root(*id) {
                        if !self.candidate_chain_has_field(root, property) {
                            self.disq_root(root, report::ESC_UNDECLARED_PROPERTY);
                        } else {
                            self.field_stores
                                .entry(root)
                                .or_default()
                                .push((property.clone(), StoreValue::Update));
                        }
                        return;
                    }
                }
                self.walk_expr(object);
            }
            Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            } => {
                if let (Expr::LocalGet(id), Expr::LocalGet(rid), Expr::String(property)) =
                    (target.as_ref(), receiver.as_ref(), key.as_ref())
                {
                    if id == rid {
                        if let Some(root) = self.tracked_root(*id) {
                            if !self.candidate_chain_has_field(root, property) {
                                self.disq_root(root, report::ESC_UNDECLARED_PROPERTY);
                            } else {
                                self.field_stores
                                    .entry(root)
                                    .or_default()
                                    .push((property.clone(), StoreValue::Direct(value)));
                            }
                            self.with_ctx(report::ESC_ELEMENT, |w| w.walk_expr(value));
                            return;
                        }
                    }
                }
                self.walk_expr(target);
                self.walk_expr(key);
                self.walk_expr(value);
                self.walk_expr(receiver);
            }
            // Method call on a tracked member: receiver-position use is safe
            // when the method is chain-resolvable; `this`-flow safety is
            // vetted in pass 3. Tracked references in ARGS still escape.
            Expr::Call { callee, args, .. } => {
                if let Expr::PropertyGet {
                    object, property, ..
                } = callee.as_ref()
                {
                    if let Expr::LocalGet(id) = object.as_ref() {
                        if let Some(root) = self.tracked_root(*id) {
                            let class_name = &self.candidates[&root];
                            let chain = chain_classes(self.classes, class_name);
                            let resolvable = chain_method_map(&chain).contains_key(property);
                            if !resolvable {
                                self.disq_root(root, report::ESC_UNRESOLVED_METHOD);
                            } else {
                                self.method_calls
                                    .entry(root)
                                    .or_default()
                                    .entry(property.clone())
                                    .or_default()
                                    .push(args.as_slice());
                            }
                            for a in args {
                                // Position-aware: `o.m(o.field)` is safe,
                                // `o.m(o)` escapes via the LocalGet arm.
                                self.with_ctx(report::ESC_CALL_ARGUMENT, |w| w.walk_expr(a));
                            }
                            return;
                        }
                    }
                }
                self.walk_expr(callee);
                for a in args {
                    self.with_ctx(report::ESC_CALL_ARGUMENT, |w| w.walk_expr(a));
                }
            }
            // A tracked member passed to a constructor escapes as an argument.
            Expr::New { args, .. } => {
                for a in args {
                    self.with_ctx(report::ESC_CALL_ARGUMENT, |w| w.walk_expr(a));
                }
            }
            // Container literals: a tracked member becomes an element.
            Expr::Array(items) => {
                for i in items {
                    self.with_ctx(report::ESC_ELEMENT, |w| w.walk_expr(i));
                }
            }
            // Barriers / hard escapes on a tracked member itself.
            Expr::Delete(inner) => {
                match inner.as_ref() {
                    Expr::PropertyGet { object, .. } | Expr::IndexGet { object, .. } => {
                        if let Expr::LocalGet(id) = object.as_ref() {
                            if self.tracked_root(*id).is_some() {
                                self.disq(*id, report::ESC_DELETE);
                                return;
                            }
                        }
                    }
                    _ => {}
                }
                self.walk_expr(inner);
            }
            Expr::ObjectFreeze(t) | Expr::ObjectSeal(t) | Expr::ObjectPreventExtensions(t) => {
                if let Expr::LocalGet(id) = t.as_ref() {
                    if self.tracked_root(*id).is_some() {
                        self.disq(*id, report::ESC_FREEZE);
                        return;
                    }
                }
                self.walk_expr(t);
            }
            // Reassignment / bare reference / numeric update = escape.
            Expr::LocalSet(id, v) => {
                self.disq(*id, report::ESC_REASSIGNED);
                self.walk_expr(v);
            }
            Expr::LocalGet(id) => {
                let why = self.escape_ctx;
                self.disq(*id, why);
            }
            Expr::Update { id, .. } => {
                self.disq(*id, report::ESC_REASSIGNED);
            }
            // Id-keyed variants the child walker cannot see.
            Expr::ArrayPush { array_id, .. }
            | Expr::ArrayPushSpread { array_id, .. }
            | Expr::ArrayUnshift { array_id, .. }
            | Expr::ArraySplice { array_id, .. }
            | Expr::ArrayCopyWithin { array_id, .. } => {
                self.disq(*array_id, report::ESC_CONTAINER_MUTATOR);
                self.with_ctx(report::ESC_ELEMENT, |w| {
                    perry_hir::walker::walk_expr_children(e, &mut |c| w.walk_expr(c))
                });
            }
            Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
                self.disq(*id, report::ESC_CONTAINER_MUTATOR);
            }
            Expr::SetAdd { set_id, .. } => {
                self.disq(*set_id, report::ESC_CONTAINER_MUTATOR);
                self.with_ctx(report::ESC_ELEMENT, |w| {
                    perry_hir::walker::walk_expr_children(e, &mut |c| w.walk_expr(c))
                });
            }
            Expr::WithSet { fallback, .. } => {
                match fallback {
                    perry_hir::WithSetFallback::Local(id)
                    | perry_hir::WithSetFallback::SloppyImplicit(id) => {
                        self.disq(*id, report::ESC_BARE_REFERENCE);
                    }
                    _ => {}
                }
                perry_hir::walker::walk_expr_children(e, &mut |c| self.walk_expr(c));
            }
            // Closures: captured or body-referenced tracked members escape
            // (capture machinery + frame lifetime).
            Expr::Closure {
                body,
                captures,
                mutable_captures,
                ..
            } => {
                for c in captures.iter().chain(mutable_captures.iter()) {
                    self.disq(*c, report::ESC_CLOSURE_CAPTURE);
                }
                let outer = self.in_closure;
                self.in_closure = true;
                self.walk_stmts(body);
                self.in_closure = outer;
            }
            // Everything else: recurse into children; a bare LocalGet of a
            // candidate in any unhandled position hits the LocalGet arm above
            // and escapes. Note: closure BODIES are Vec<Stmt>, handled above;
            // walk_expr_children yields only Expr children.
            _ => {
                perry_hir::walker::walk_expr_children(e, &mut |c| self.walk_expr(c));
            }
        }
    }
}

// ── Pass 3: `this`-flow safety of constructors and called methods ──────────

/// A `this.field = value` store observed inside a constructor, a field
/// initializer, or a method body, with the owning function's parameter ids so
/// parameter-mediated values can be resolved through call-site arguments.
struct ThisStoreRecord<'a> {
    field: String,
    /// `None` = `++`/`--` update (numeric); `Some` = the stored expression.
    value: Option<&'a Expr>,
    /// Owning context: `None` for field initializers; `Some((owner_class,
    /// method_name, param_ids))` for constructor ("constructor") and methods.
    context: Option<(String, String, Vec<u32>)>,
}

pub(super) struct ThisFlowAnalysis<'a, 'b> {
    chain: &'b [&'a Class],
    fields: &'b HashSet<String>,
    methods: &'b HashMap<String, (String, &'a perry_hir::Function)>,
    visited: HashSet<(String, String)>,
    store_records: Vec<ThisStoreRecord<'a>>,
    /// `super(...)` argument lists observed in chain constructors, keyed by
    /// the PARENT (callee) class name. Feeds the parent-ctor parameter
    /// resolution of the numeric-field proof.
    super_call_args: HashMap<String, Vec<&'a [Expr]>>,
    /// Method names invoked INTERNALLY — `this.m(...)` from a constructor or
    /// another method, and `super.m(...)`. Their argument expressions live in
    /// the CALLING method's scope, which the numeric-field proof's
    /// `ParamEnv::Sites` (function-scope call-site args) cannot resolve —
    /// so parameters of internally-invoked methods must stay unproven even
    /// when every EXTERNAL call site passes numeric arguments.
    internally_invoked: HashSet<String>,
    /// Phase 5a only: permit a `this.f = <expr mentioning this>` store.
    ///
    /// Phase 3b rejects those because its numeric-field proof resolves store
    /// VALUES through constructor/method call-site arguments, and a
    /// `this`-dependent value cannot be resolved that way — so the store must
    /// not be recorded as provably-numeric. Phase 5a claims no numeric fields
    /// at all (`collectors/proven_this.rs`), so the restriction buys it
    /// nothing while excluding the single most common method shape there is:
    /// `this.value = this.value + 1`. Safety is unaffected — the value
    /// expression still goes through `expr_this_safe`, which rejects `this`
    /// in value position and admits only declared-chain `this.field` reads.
    allow_this_in_store_values: bool,
}

impl<'a, 'b> ThisFlowAnalysis<'a, 'b> {
    /// A fresh analysis over one class chain. Phase 5a
    /// (`collectors/proven_this.rs`) reuses the walk for a method's `this`
    /// without the constructor-chain obligations: the receiver of a proven
    /// `this` already exists, and its shape is established by the CALL SITE
    /// guard (class id + keys token) rather than by in-function provenance.
    pub(super) fn new(
        chain: &'b [&'a Class],
        fields: &'b HashSet<String>,
        methods: &'b HashMap<String, (String, &'a perry_hir::Function)>,
    ) -> Self {
        Self {
            chain,
            fields,
            methods,
            visited: HashSet::new(),
            store_records: Vec::new(),
            super_call_args: HashMap::new(),
            internally_invoked: HashSet::new(),
            allow_this_in_store_values: true,
        }
    }

    /// Did the vetted walk observe any `this.<field> = …` store (in this
    /// method or anything it transitively invokes on the same `this`)?
    /// Phase 5a gates the freeze-family kill on this.
    pub(super) fn has_this_store_records(&self) -> bool {
        self.store_records.iter().any(|r| r.context.is_some())
    }

    /// Walk the constructor chain (self-first `super(...)` order) and every
    /// chain field initializer under the strict `this` discipline.
    fn ctor_chain_safe(&mut self) -> bool {
        for class in self.chain {
            for field in &class.fields {
                if let Some(init) = &field.init {
                    if expr_mentions_this(init) {
                        return false;
                    }
                    self.store_records.push(ThisStoreRecord {
                        field: field.name.clone(),
                        value: Some(init),
                        context: None,
                    });
                }
            }
        }
        for class in self.chain {
            if let Some(ctor) = &class.constructor {
                if !self.function_this_safe(&class.name, "constructor", ctor) {
                    return false;
                }
            }
        }
        true
    }

    pub(super) fn method_safe(&mut self, owner: &str, func: &'a perry_hir::Function) -> bool {
        self.function_this_safe(owner, &func.name, func)
    }

    fn function_this_safe(
        &mut self,
        owner: &str,
        name: &str,
        func: &'a perry_hir::Function,
    ) -> bool {
        let key = (owner.to_string(), name.to_string());
        if !self.visited.insert(key) {
            return true; // already vetted (or in-progress higher up the stack)
        }
        if self.visited.len() > 64 {
            return false;
        }
        if func.is_async || func.is_generator || func.was_plain_async {
            return false;
        }
        let param_ids: Vec<u32> = func.params.iter().map(|p| p.id).collect();
        let ctx = (owner.to_string(), name.to_string(), param_ids);
        let mut safe = true;
        for s in &func.body {
            if !safe {
                break;
            }
            safe &= self.stmt_this_safe(s, &ctx);
        }
        safe
    }

    fn stmt_this_safe(&mut self, s: &'a Stmt, ctx: &(String, String, Vec<u32>)) -> bool {
        match s {
            Stmt::Let { init, .. } => init
                .as_ref()
                .map(|e| self.expr_this_safe(e, ctx))
                .unwrap_or(true),
            Stmt::Expr(e) | Stmt::Throw(e) => self.expr_this_safe(e, ctx),
            Stmt::Return(opt) => {
                // A constructor `return <expr>` can OVERRIDE the `new` result
                // (`js_ctor_return_override`): the provenance proof "the local
                // holds exactly a C instance" would be wrong. Disqualify any
                // value-returning chain constructor (conservative — even
                // primitive returns, which JS ignores).
                if ctx.1 == "constructor" && opt.is_some() {
                    return false;
                }
                opt.as_ref()
                    .map(|e| self.expr_this_safe(e, ctx))
                    .unwrap_or(true)
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr_this_safe(condition, ctx)
                    && then_branch.iter().all(|s| self.stmt_this_safe(s, ctx))
                    && else_branch
                        .as_ref()
                        .map(|b| b.iter().all(|s| self.stmt_this_safe(s, ctx)))
                        .unwrap_or(true)
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.expr_this_safe(condition, ctx)
                    && body.iter().all(|s| self.stmt_this_safe(s, ctx))
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref()
                    .map(|s| self.stmt_this_safe(s, ctx))
                    .unwrap_or(true)
                    && condition
                        .as_ref()
                        .map(|e| self.expr_this_safe(e, ctx))
                        .unwrap_or(true)
                    && update
                        .as_ref()
                        .map(|e| self.expr_this_safe(e, ctx))
                        .unwrap_or(true)
                    && body.iter().all(|s| self.stmt_this_safe(s, ctx))
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                body.iter().all(|s| self.stmt_this_safe(s, ctx))
                    && catch
                        .as_ref()
                        .map(|c| c.body.iter().all(|s| self.stmt_this_safe(s, ctx)))
                        .unwrap_or(true)
                    && finally
                        .as_ref()
                        .map(|f| f.iter().all(|s| self.stmt_this_safe(s, ctx)))
                        .unwrap_or(true)
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr_this_safe(discriminant, ctx)
                    && cases.iter().all(|case| {
                        case.test
                            .as_ref()
                            .map(|t| self.expr_this_safe(t, ctx))
                            .unwrap_or(true)
                            && case.body.iter().all(|s| self.stmt_this_safe(s, ctx))
                    })
            }
            Stmt::Labeled { body, .. } => self.stmt_this_safe(body.as_ref(), ctx),
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_) => true,
        }
    }

    fn expr_this_safe(&mut self, e: &'a Expr, ctx: &(String, String, Vec<u32>)) -> bool {
        match e {
            Expr::PropertyGet {
                object, property, ..
            } if matches!(object.as_ref(), Expr::This) => self.fields.contains(property),
            Expr::PropertySet {
                object,
                property,
                value,
            } if matches!(object.as_ref(), Expr::This) => {
                if !self.fields.contains(property)
                    || (!self.allow_this_in_store_values && expr_mentions_this(value))
                {
                    return false;
                }
                self.store_records.push(ThisStoreRecord {
                    field: property.clone(),
                    value: Some(value),
                    context: Some(ctx.clone()),
                });
                self.expr_this_safe(value, ctx)
            }
            Expr::PropertyUpdate {
                object, property, ..
            } if matches!(object.as_ref(), Expr::This) => {
                if !self.fields.contains(property) {
                    return false;
                }
                self.store_records.push(ThisStoreRecord {
                    field: property.clone(),
                    value: None,
                    context: Some(ctx.clone()),
                });
                true
            }
            Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            } if matches!(target.as_ref(), Expr::This)
                && matches!(receiver.as_ref(), Expr::This) =>
            {
                let Expr::String(property) = key.as_ref() else {
                    return false;
                };
                if !self.fields.contains(property)
                    || (!self.allow_this_in_store_values && expr_mentions_this(value))
                {
                    return false;
                }
                self.store_records.push(ThisStoreRecord {
                    field: property.clone(),
                    value: Some(value),
                    context: Some(ctx.clone()),
                });
                self.expr_this_safe(value, ctx)
            }
            // `this.m(args)` — vet the callee method transitively.
            Expr::Call { callee, args, .. }
                if matches!(
                    callee.as_ref(),
                    Expr::PropertyGet { object, .. } if matches!(object.as_ref(), Expr::This)
                ) =>
            {
                let Expr::PropertyGet { property, .. } = callee.as_ref() else {
                    unreachable!()
                };
                if self.fields.contains(property) {
                    return false; // calling a field-held closure: dynamic
                }
                let Some((owner, func)) = self.methods.get(property).cloned() else {
                    return false;
                };
                self.internally_invoked.insert(property.clone());
                if !self.function_this_safe(&owner, property, func) {
                    return false;
                }
                args.iter()
                    .all(|a| !expr_mentions_this(a) && self.expr_this_safe(a, ctx))
            }
            // `super(...)`: the parent constructor body was already vetted by
            // `ctor_chain_safe` (whole chain). Args must not leak `this`; in
            // constructor context, record them for parent-ctor parameter
            // resolution in the numeric-field proof.
            Expr::SuperCall(args) => {
                if ctx.1 == "constructor" {
                    if let Some(pos) = self.chain.iter().position(|c| c.name == ctx.0) {
                        if let Some(parent) = self.chain.get(pos + 1) {
                            self.super_call_args
                                .entry(parent.name.clone())
                                .or_default()
                                .push(args.as_slice());
                        }
                    }
                }
                args.iter()
                    .all(|a| !expr_mentions_this(a) && self.expr_this_safe(a, ctx))
            }
            // `super.m(...)` resolves on the parent chain with the same `this`.
            Expr::SuperMethodCall { method, args, .. } => {
                let Some((owner, func)) = self.methods.get(method).cloned() else {
                    return false;
                };
                self.internally_invoked.insert(method.clone());
                if !self.function_this_safe(&owner, method, func) {
                    return false;
                }
                args.iter()
                    .all(|a| !expr_mentions_this(a) && self.expr_this_safe(a, ctx))
            }
            // Shape barriers on `this` inside a method body (the module-wide
            // kill already covers these; kept as defense in depth).
            Expr::Delete(inner)
                if matches!(
                    inner.as_ref(),
                    Expr::PropertyGet { object, .. } | Expr::IndexGet { object, .. }
                        if matches!(object.as_ref(), Expr::This)
                ) =>
            {
                false
            }
            // Any other appearance of `this` — including inside closures —
            // is a potential leak.
            Expr::This => false,
            Expr::Closure { body, .. } => {
                // A closure that touches `this` (captures_this or body use)
                // leaks it; a `this`-free closure is fine but its body may
                // reference nothing we track here (locals are the outer
                // function's problem — the use walk already handled the
                // candidate local itself).
                if expr_mentions_this(e) {
                    return false;
                }
                let _ = body;
                true
            }
            _ => {
                let mut ok = true;
                perry_hir::walker::walk_expr_children(e, &mut |c| {
                    if ok {
                        ok = self.expr_this_safe(c, ctx);
                    }
                });
                ok
            }
        }
    }
}

/// Does the expression mention `this` anywhere (including closure bodies and
/// `captures_this`)?
fn expr_mentions_this(e: &Expr) -> bool {
    let mut found = false;
    fn visit(e: &Expr, found: &mut bool) {
        if *found {
            return;
        }
        match e {
            Expr::This => {
                *found = true;
            }
            Expr::Closure {
                body,
                captures_this,
                ..
            } => {
                if *captures_this {
                    *found = true;
                    return;
                }
                for s in body {
                    stmt_visit(s, found);
                }
            }
            _ => {}
        }
        if *found {
            return;
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| visit(c, found));
    }
    fn stmt_visit(s: &Stmt, found: &mut bool) {
        if *found {
            return;
        }
        match s {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    visit(e, found);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => visit(e, found),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                visit(condition, found);
                for s in then_branch {
                    stmt_visit(s, found);
                }
                if let Some(eb) = else_branch {
                    for s in eb {
                        stmt_visit(s, found);
                    }
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                visit(condition, found);
                for s in body {
                    stmt_visit(s, found);
                }
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    stmt_visit(i, found);
                }
                if let Some(c) = condition {
                    visit(c, found);
                }
                if let Some(u) = update {
                    visit(u, found);
                }
                for s in body {
                    stmt_visit(s, found);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for s in body {
                    stmt_visit(s, found);
                }
                if let Some(c) = catch {
                    for s in &c.body {
                        stmt_visit(s, found);
                    }
                }
                if let Some(f) = finally {
                    for s in f {
                        stmt_visit(s, found);
                    }
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                visit(discriminant, found);
                for case in cases {
                    if let Some(t) = &case.test {
                        visit(t, found);
                    }
                    for s in &case.body {
                        stmt_visit(s, found);
                    }
                }
            }
            Stmt::Labeled { body, .. } => stmt_visit(body.as_ref(), found),
            _ => {}
        }
    }
    visit(e, &mut found);
    found
}

// ── Pass 4: numeric-proven fields ──────────────────────────────────────────

/// Greatest-fixpoint proof that every reachable store into a raw-f64-declared
/// chain field is number-producing. Parameter-mediated stores resolve through
/// the actual argument expressions at the provenance `new` (constructor) or
/// at every recorded call site (methods).
/// Parameter environment for [`expr_numeric_by_construction`].
enum ParamEnv<'x> {
    /// Function scope: no parameters; const-local chasing applies.
    None,
    /// Method scope: params resolve through recorded call-site argument
    /// lists (each argument evaluated in function scope).
    Sites {
        param_ids: &'x [u32],
        sites: Vec<&'x [Expr]>,
    },
    /// Constructor scope: params pre-resolved to a numeric verdict through
    /// the provenance `new` / `super(...)` argument chain.
    Resolved(&'x HashMap<u32, bool>),
}

#[allow(clippy::too_many_arguments)]
fn prove_numeric_fields(
    chain: &[&Class],
    members: &HashSet<u32>,
    this_stores: &[ThisStoreRecord<'_>],
    local_stores: &[(String, StoreValue<'_>)],
    new_args: &[Expr],
    method_calls: Option<&HashMap<String, Vec<&[Expr]>>>,
    super_call_args: &HashMap<String, Vec<&[Expr]>>,
    internally_invoked: &HashSet<String>,
    not_bigint_locals: &HashSet<u32>,
    const_local_inits: &HashMap<u32, Option<&Expr>>,
) -> HashSet<String> {
    let mut numeric: HashSet<String> = HashSet::new();
    for class in chain {
        for field in &class.fields {
            if crate::typed_shape::type_is_raw_f64_candidate(&field.ty) {
                numeric.insert(field.name.clone());
            }
        }
    }
    if numeric.is_empty() {
        return numeric;
    }
    // Resolve the argument expressions that can flow into a given
    // (context, param position): the provenance `new` args feed the root
    // constructor; each parent constructor's params resolve through the
    // recorded `super(...)` argument lists, evaluated under the CALLING
    // constructor's (already-resolved) parameter environment. Derived-first
    // chain order makes this a single top-down pass. The environment is
    // computed against an EMPTY numeric-field set (strictly conservative —
    // `super(this.x)` cannot occur, `this` is banned in super args).
    let mut ctor_param_env: HashMap<String, HashMap<u32, bool>> = HashMap::new();
    {
        let empty_numeric: HashSet<String> = HashSet::new();
        for (pos, class) in chain.iter().enumerate() {
            let Some(ctor) = class.constructor.as_ref() else {
                continue;
            };
            let mut env: HashMap<u32, bool> = HashMap::new();
            if pos == 0 {
                for (i, param) in ctor.params.iter().enumerate() {
                    let ok = new_args
                        .get(i)
                        .map(|a| {
                            expr_numeric_by_construction(
                                a,
                                &ParamEnv::None,
                                members,
                                &empty_numeric,
                                not_bigint_locals,
                                const_local_inits,
                                0,
                            )
                        })
                        .unwrap_or(false);
                    env.insert(param.id, ok);
                }
            } else {
                let caller_env = chain
                    .get(pos - 1)
                    .and_then(|caller| ctor_param_env.get(caller.name.as_str()));
                let lists = super_call_args.get(class.name.as_str());
                for (i, param) in ctor.params.iter().enumerate() {
                    let ok = match (lists, caller_env) {
                        (Some(lists), Some(caller_env)) if !lists.is_empty() => {
                            lists.iter().all(|args| {
                                args.get(i)
                                    .map(|a| {
                                        expr_numeric_by_construction(
                                            a,
                                            &ParamEnv::Resolved(caller_env),
                                            members,
                                            &empty_numeric,
                                            not_bigint_locals,
                                            const_local_inits,
                                            0,
                                        )
                                    })
                                    .unwrap_or(false)
                            })
                        }
                        _ => false,
                    };
                    env.insert(param.id, ok);
                }
            }
            ctor_param_env.insert(class.name.clone(), env);
        }
    }

    loop {
        let before = numeric.len();
        let is_store_numeric = |field: &str,
                                value: Option<&Expr>,
                                context: Option<&(String, String, Vec<u32>)>,
                                numeric: &HashSet<String>|
         -> bool {
            let _ = field;
            let Some(value) = value else {
                // `++`/`--` — ToNumeric of a proven-number field stays a
                // number; if the field is currently claimed numeric the
                // update preserves it.
                return true;
            };
            let param_env: ParamEnv<'_> = match context {
                None => ParamEnv::None,
                Some((owner, name, param_ids)) => {
                    if name == "constructor" {
                        match ctor_param_env.get(owner.as_str()) {
                            Some(env) => ParamEnv::Resolved(env),
                            None => ParamEnv::Sites {
                                param_ids: param_ids.as_slice(),
                                sites: Vec::new(),
                            },
                        }
                    } else {
                        // A method that is ALSO invoked internally
                        // (`this.m(...)` / `super.m(...)`) receives argument
                        // expressions from method scope that the
                        // function-scope site resolution below cannot see —
                        // its parameters stay unproven even when every
                        // external site is numeric (an internal
                        // `this.m("s")` would otherwise poison a
                        // "proven" field). Purely-external methods resolve
                        // through their recorded call sites; purely-internal
                        // ones have no sites and stay unproven either way.
                        let sites: Vec<&[Expr]> = if internally_invoked.contains(name.as_str()) {
                            Vec::new()
                        } else {
                            method_calls
                                .and_then(|mc| mc.get(name))
                                .map(|v| v.clone())
                                .unwrap_or_default()
                        };
                        ParamEnv::Sites {
                            param_ids: param_ids.as_slice(),
                            sites,
                        }
                    }
                }
            };
            expr_numeric_by_construction(
                value,
                &param_env,
                members,
                numeric,
                not_bigint_locals,
                const_local_inits,
                0,
            )
        };
        // Field initializers + ctor/method stores.
        let mut retained: HashSet<String> = numeric.clone();
        for rec in this_stores {
            if retained.contains(&rec.field)
                && !is_store_numeric(&rec.field, rec.value, rec.context.as_ref(), &numeric)
            {
                retained.remove(&rec.field);
            }
        }
        for (field, sv) in local_stores {
            if retained.contains(field) {
                let ok = match sv {
                    StoreValue::Update => true,
                    StoreValue::Direct(v) => expr_numeric_by_construction(
                        v,
                        &ParamEnv::None,
                        members,
                        &numeric,
                        not_bigint_locals,
                        const_local_inits,
                        0,
                    ),
                };
                if !ok {
                    retained.remove(field);
                }
            }
        }
        numeric = retained;
        if numeric.len() == before || numeric.is_empty() {
            break;
        }
    }
    numeric
}

/// Number-by-construction: the expression's runtime value is a JS Number for
/// every input, per spec — never a string/BigInt/bool/undefined/pointer.
fn expr_numeric_by_construction(
    e: &Expr,
    param_env: &ParamEnv<'_>,
    members: &HashSet<u32>,
    numeric_fields: &HashSet<String>,
    not_bigint_locals: &HashSet<u32>,
    const_local_inits: &HashMap<u32, Option<&Expr>>,
    depth: usize,
) -> bool {
    if depth > 16 {
        return false;
    }
    use perry_hir::BinaryOp;
    let rec = |x: &Expr| {
        expr_numeric_by_construction(
            x,
            param_env,
            members,
            numeric_fields,
            not_bigint_locals,
            const_local_inits,
            depth + 1,
        )
    };
    match e {
        Expr::Number(_) | Expr::Integer(_) => true,
        Expr::Unary { op, operand } => match op {
            perry_hir::UnaryOp::Neg | perry_hir::UnaryOp::Pos | perry_hir::UnaryOp::BitNot => {
                rec(operand)
            }
            _ => false,
        },
        Expr::Binary { op, left, right } => match op {
            // `+` concatenates strings; both sides must be numbers.
            BinaryOp::Add => rec(left) && rec(right),
            // `- * / %` produce BigInt only for BigInt⊗BigInt; a provably
            // non-BigInt operand forces the Number path.
            // `- * / %` produce a BigInt only for BigInt⊗BigInt; mixing a
            // BigInt with anything else THROWS (no value is stored). ONE
            // provably-non-BigInt operand therefore forces the completed
            // result onto the Number path.
            BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                (rec(left) && rec(right))
                    || expr_provably_not_bigint(left, not_bigint_locals)
                    || expr_provably_not_bigint(right, not_bigint_locals)
            }
            // Same either-side argument for the BigInt-capable bitwise ops.
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr => {
                (rec(left) && rec(right))
                    || expr_provably_not_bigint(left, not_bigint_locals)
                    || expr_provably_not_bigint(right, not_bigint_locals)
            }
            // `>>>` throws for BigInt operands; result is always a Number.
            BinaryOp::UShr => true,
            _ => false,
        },
        Expr::NumberCoerce(_)
        | Expr::ParseFloat(_)
        | Expr::ParseInt { .. }
        | Expr::MathSqrt(_)
        | Expr::MathFloor(_)
        | Expr::MathCeil(_)
        | Expr::MathRound(_)
        | Expr::MathTrunc(_)
        | Expr::MathSign(_)
        | Expr::MathAbs(_)
        | Expr::MathF16round(_)
        | Expr::MathPow(..)
        | Expr::MathMin(_)
        | Expr::MathMax(_)
        | Expr::MathMinSpread(_)
        | Expr::MathMaxSpread(_)
        | Expr::DateNow
        | Expr::PerformanceNow => true,
        // A proven-numeric field of the SAME object (fixpoint edge): `this`
        // inside the candidate's ctor/method contexts (a non-None param env),
        // or a tracked member local in function scope. A same-named field of
        // a DIFFERENT object proves nothing.
        Expr::PropertyGet {
            object, property, ..
        } if match object.as_ref() {
            Expr::This => !matches!(param_env, ParamEnv::None),
            Expr::LocalGet(id) => members.contains(id),
            _ => false,
        } =>
        {
            numeric_fields.contains(property)
        }
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => rec(then_expr) && rec(else_expr),
        Expr::Sequence(es) => es.last().map(|x| rec(x)).unwrap_or(false),
        // A parameter: numeric iff every recorded call site passes a numeric
        // argument at that position (missing argument = `undefined`, not
        // numeric). No recorded sites = unproven.
        Expr::LocalGet(id) => {
            match param_env {
                ParamEnv::Sites { param_ids, sites } => {
                    if let Some(pos) = param_ids.iter().position(|p| p == id) {
                        return !sites.is_empty()
                            && sites.iter().all(|args| {
                                args.get(pos).map(|a| {
                                    expr_numeric_by_construction(
                                        a,
                                        &ParamEnv::None,
                                        members,
                                        numeric_fields,
                                        not_bigint_locals,
                                        const_local_inits,
                                        depth + 1,
                                    )
                                }) == Some(true)
                            });
                    }
                }
                ParamEnv::Resolved(env) => {
                    if let Some(&ok) = env.get(id) {
                        return ok;
                    }
                }
                ParamEnv::None => {
                    // A single-Let const temp: chase its init (function
                    // scope, so no parameter mapping applies to it).
                    if let Some(Some(init)) = const_local_inits.get(id) {
                        return expr_numeric_by_construction(
                            init,
                            &ParamEnv::None,
                            members,
                            numeric_fields,
                            not_bigint_locals,
                            const_local_inits,
                            depth + 1,
                        );
                    }
                }
            }
            false
        }
        _ => false,
    }
}

/// Conservative "cannot be a BigInt" for the spec Number-path argument.
fn expr_provably_not_bigint(e: &Expr, not_bigint_locals: &HashSet<u32>) -> bool {
    match e {
        Expr::Number(_) | Expr::Integer(_) | Expr::String(_) | Expr::Bool(_) => true,
        Expr::LocalGet(id) => not_bigint_locals.contains(id),
        Expr::Unary { op, operand } => match op {
            perry_hir::UnaryOp::Pos => true, // `+x` throws for BigInt
            perry_hir::UnaryOp::Neg | perry_hir::UnaryOp::BitNot => {
                expr_provably_not_bigint(operand, not_bigint_locals)
            }
            _ => true, // !x, typeof x, … never produce BigInt
        },
        Expr::Binary { .. } => false, // handled structurally by the caller
        _ => false,
    }
}

// Note on Symbol operands in the either-side non-BigInt arithmetic argument:
// ToNumber(Symbol) THROWS, so the store never completes — throw behavior is
// identical on the guarded and bare paths, and no non-number value can reach
// the slot through these operators.

/// `--opt-report` (#6952) end-to-end tests. Kept in a sibling file for the
/// file-size gate; still a child module, so `use super::*` reaches the
/// collector's private items.
#[cfg(test)]
#[path = "ptr_shape_opt_report_tests.rs"]
mod opt_report_tests;
