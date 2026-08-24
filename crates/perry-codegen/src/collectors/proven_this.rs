//! Representation-selection Phase 5a (RFC `docs/representation-selection-rfc.md`
//! §4 Object row, §5.4, §5.6): the **proven `this`** of a class method.
//!
//! ## The proof already exists — this phase stops discarding it
//!
//! Two call sites already prove a receiver's exact shape and then call the
//! guard-ridden PUBLIC method body, where every `this.field` re-enters the
//! per-access guarded diamond (`expr/class_field_inline_guard.rs`) whose cost
//! is unhoistable by construction (volatile gate + the fallback arm's opaque
//! call block LICM):
//!
//! 1. `lower_call/method_override.rs` — the `method_direct.fast` arm, entered
//!    only after `js_typed_feedback_method_direct_call_guard` /
//!    `js_method_direct_shape_guard` matched **class_id AND the keys token**.
//! 2. `lower_call/property_get/dynamic_dispatch.rs` — the Phase 3b guard-free
//!    `Ptr<Shape>` receiver arm, whose receiver is a shape-proven local.
//!
//! Phase 5a emits a `{public}$pshape` clone of the method whose
//! `this` carries the [`PtrShapeLocal`] proof, and routes those two sites to
//! it. Net new proof work: zero. Net new GC work: zero — the clone keeps the
//! identical `(double this, double args…)` ABI and the identical shadow-bound
//! tagged-at-rest receiver slot (`codegen/method.rs`), because `GC_TYPE_OBJECT`
//! is MOVABLE and the `TaPtr` no-shadow shortcut does not transfer. Net new
//! layout work: zero — field indexes come from `class_field_global_index`,
//! which is chain-global (parent fields first).
//!
//! ## Why the receiver is exactly this class
//!
//! `this` inside `C.prototype.m` can in general be an instance of any SUBCLASS
//! of `C`, which would invalidate a proof rooted at `C`'s chain. Phase 5a
//! sidesteps this entirely at the ROUTING sites rather than here: both of them
//! only route when the receiver's proven exact class **declares** the method
//! (`ctx.methods` holds own-declarations only — `method_registry.rs` never
//! inserts inherited entries), so the class the clone was compiled for is the
//! receiver's exact dynamic class. Inherited dispatch (`d.m()` resolving to
//! `Base::m`) keeps today's lowering.
//!
//! ## `numeric_fields` is deliberately NOT claimed
//!
//! A Phase 3b local claims `JsNumber`/`F64` per field under an EXHAUSTIVE
//! reachable-store proof, which containment makes possible: no alias to the
//! object exists, so the analysis sees every store. A proven `this` is owned by
//! the CALLER and is aliased by construction — `counter.value = "s"` elsewhere
//! is a reachable store this analysis cannot enumerate, and it downgrades the
//! slot's raw-f64 layout at runtime. A guard-free read cannot consult that
//! layout, so claiming `JsNumber` would be unsound. Phase 5a therefore emits
//! bare `load double` with generic `JsValue` semantics — still fully
//! guard-free, and bit-identical (a NaN-boxed number IS its own double bits).
//! Recovering the numeric claim needs a whole-program no-external-store proof;
//! that is deferred.
//!
//! ## `delete` is aliased across modules by construction (#7143) — closed, not a bug
//!
//! [`method_proven_this`] below consults `ModuleDispatchFacts::has_shape_barrier_sites`,
//! which `collect_module_dispatch_facts`
//! (`collectors/scalar_method_dispatch.rs`) computes **per module**. A
//! `delete` / `Reflect.deleteProperty` in a module
//! that never declares `class_name` sets no flag this admission check can
//! see. Unlike a Phase 3b `Ptr<Shape>` LOCAL — whose containment (rule 2,
//! `ptr_shape.rs`) proves no alias to the object can exist ANYWHERE, in this
//! module or any other — a proven `this` is the caller's object and is
//! aliased by construction: it can be handed to another module, deleted
//! from there, and handed back.
//!
//! This is sound anyway. Every routing site that can call a `$pshape` clone
//! re-derives the guarantee itself, at the point it actually matters, rather
//! than trusting this admission-time fact to have seen the whole program:
//!
//! * `method_direct.fast` (`lower_call/method_override.rs`) sits behind
//!   `js_method_direct_shape_guard` / `js_typed_feedback_method_direct_call_guard`,
//!   whose contract includes `receiver.ShapeId == expected_shape_id`
//!   (`typed_feedback/guards.rs`). `js_object_delete_field` publishes a
//!   semantic successor ShapeId (`perry-runtime/src/object/delete_rest.rs`;
//!   `Reflect.deleteProperty` shares the same function), so the guard can
//!   never pass on a post-delete instance regardless of module boundaries.
//! * The Phase 3b guard-free `Ptr<Shape>` receiver arm needs no runtime
//!   check at all, because rule 2's containment already rules out the alias
//!   existing in the first place: creating one — `let other = o`, passing
//!   `o` to ANY function, same module or not — is itself a disqualifying
//!   use the containment walk sees directly. There is no alias left for a
//!   `delete` anywhere to reach the object through.
//! * #7142's class-id dispatch-tower case
//!   (`lower_call/property_get/dynamic_dispatch.rs::emit_tower_pshape_call`)
//!   carries its own explicit re-check
//!   (`class_field_inline_guard::emit_proven_shape_recheck`) for exactly
//!   this reason — that function's doc comment cites this issue by number: a
//!   static, module-scoped proof would have been "exactly the wrong
//!   instrument" for a receiver that can be aliased across modules.
//!
//! So `has_shape_barrier_sites()` here is a **cost-control heuristic** —
//! whether emitting a clone is even worth it, given the module's own code
//! may never take a fast path to it — never the mechanism that makes routing
//! to one safe. A future 4th routing site must independently re-derive one
//! of the two guarantees above (a dominating keys-token recheck, or genuine
//! containment); it must NOT rely on this fact having seen a `delete` that,
//! by construction, may have happened in a module this one never looked at.
//!
//! Confirmed empirically, not just by proof-reading:
//! `test-files/test_issue_7143_delete_barrier_cross_module.ts` (+
//! `test-files/fixtures/issue_7143_pkg/shared.ts`) is exactly this shape — a
//! `delete` in the importing module, then a call back into the declaring
//! module on the mutated instance — and the emitted `--trace llvm` IR shows
//! the `$pshape` call dominated by `js_typed_feedback_method_direct_call_guard`
//! as described above; the compiled binary's output matches
//! `node --experimental-strip-types` exactly. The
//! `guarded_pshape_call_site_is_preceded_by_a_keys_token_guard` test in
//! `proven_this_routing_tests.rs` pins the same invariant at the IR level so
//! a future change to the routing sites can't silently drop it.
//!
//! Gated by `PERRY_PTR_SHAPE_THIS` (default on; `0`/`off`/`false` disables —
//! keyed into the object cache). Also honours `PERRY_PTR_SHAPE_LOCALS`, since
//! Phase 5a is an extension of the same `Ptr<Shape>` proof.

use std::collections::{HashMap, HashSet};

use perry_hir::types::Type;
use perry_hir::{Class, Expr, Function, Stmt};

use super::ptr_shape::{
    chain_admissible, chain_classes, chain_field_names, chain_method_map, ptr_shape_locals_enabled,
    PtrShapeLocal, ThisFlowAnalysis,
};
use super::ModuleDispatchFacts;

/// `PERRY_PTR_SHAPE_THIS` gate. Enabled by default; `=0`/`off`/`false`
/// disables proven-`this` method clones (every `this.field` keeps today's
/// guarded lowering). Keyed into the object cache (`object_cache.rs`).
pub fn ptr_shape_this_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_PTR_SHAPE_THIS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// The `internal` proven-`this` clone symbol of a method. Same
/// `(double this, double args…)` ABI as the public symbol — only the body's
/// `this.field` lowering differs.
///
/// The `$` separator puts the clone in the reserved generated-suffix
/// namespace (issue #6927): `sanitize`/`sanitize_member` outputs are strictly
/// `[A-Za-z0-9_]`, so no user member — not even one literally named
/// `foo__pshape` or `foo$pshape` — can compose a public symbol equal to a
/// clone symbol. (The old `__pshape` suffix was forgeable and needed a
/// composed-symbol collision prune here.)
///
/// This symbol is NEVER registered into a runtime vtable
/// (`js_register_class_method` keeps the public name). It is externally
/// visible solely so an importing native module can use it after receiving a
/// producer-authored capability; every call remains one of the proven direct
/// sites. [`tests::pshape_symbol_reachability`] ratchets that.
pub(crate) fn pshape_method_name(public_name: &str) -> String {
    format!("{public_name}$pshape")
}

/// The contained-receiver-only array-field-cache clone.
///
/// Unlike [`pshape_method_name`], this clone must never be selected merely
/// because a call site has re-checked an aliased receiver's shape. Its body
/// keeps array-valued `this.field` loads in locals across calls in the method,
/// which requires Phase 3b's stronger provenance + containment proof. The
/// sole routing site is therefore the guard-free `Ptr<Shape>` local arm in
/// `lower_call/property_get/dynamic_dispatch.rs`.
pub(crate) fn ptr_array_cache_method_name(public_name: &str) -> String {
    format!("{public_name}$ptr_arrays")
}

/// Build a method body that snapshots stable, array-valued fields of a
/// contained receiver into immutable locals at entry.
///
/// This is deliberately a separate clone rather than a change to `$pshape`:
/// exact shape alone fixes field *offsets*, not field *values*. An aliased
/// receiver could have one of its array slots replaced by a callback while a
/// method is running. A Phase 3b local has the extra containment proof that
/// rules that alias out, and the restrictions in [`ptr_array_cache_fields`]
/// reject direct slot replacement and internally-dispatched `this` calls.
pub(crate) fn ptr_array_cached_method(class: &Class, method: &Function) -> Option<Function> {
    let fields = ptr_array_cache_fields(class, method);
    if fields.is_empty() {
        return None;
    }

    let mut used_ids = HashSet::new();
    super::collect_let_ids(&method.body, &mut used_ids);
    super::collect_ref_ids_in_stmts(&method.body, &mut used_ids);
    used_ids.extend(method.params.iter().map(|p| p.id));
    used_ids.extend(method.captures.iter().copied());
    let mut next_id = used_ids.iter().copied().max().unwrap_or(0);

    let mut aliases: HashMap<String, (u32, Type)> = HashMap::new();
    for (name, ty) in fields {
        loop {
            next_id = next_id.checked_add(1)?;
            if used_ids.insert(next_id) {
                break;
            }
        }
        aliases.insert(name, (next_id, ty));
    }

    let mut cached = method.clone();
    rewrite_array_field_reads_in_stmts(&mut cached.body, &aliases);

    // Preserve class declaration order. Apart from making generated IR stable,
    // this matches the order in which the source-level equivalent would bind
    // the aliases.
    let mut prefix = Vec::with_capacity(aliases.len());
    for field in &class.fields {
        let Some((id, ty)) = aliases.get(&field.name) else {
            continue;
        };
        prefix.push(Stmt::Let {
            id: *id,
            name: format!("__perry_ptr_array_{}", field.name),
            ty: ty.clone(),
            mutable: false,
            init: Some(Expr::PropertyGet {
                object: Box::new(Expr::This),
                property: field.name.clone(),
                byte_offset: 0,
            }),
        });
    }
    prefix.append(&mut cached.body);
    cached.body = prefix;
    Some(cached)
}

/// Array fields worth caching for [`ptr_array_cached_method`]. Empty means no
/// clone may be emitted or routed to.
///
/// The optimization is intentionally narrow:
///
/// * the method contains a loop (otherwise the extra roots/code size do not
///   amortize);
/// * the field is an own, statically-named `Array` initialized by an array
///   literal and is read through `this` in the method;
/// * the method neither replaces/deletes that field nor invokes another
///   method with the same `this` (which could replace it transitively).
///
/// Array *contents* may still change. The alias holds the same array object,
/// so pushes and indexed stores remain observable exactly as before.
pub(crate) fn ptr_array_cache_fields(class: &Class, method: &Function) -> Vec<(String, Type)> {
    if !stmts_contain_loop(&method.body) {
        return Vec::new();
    }

    let candidates: HashMap<&str, &Type> = class
        .fields
        .iter()
        .filter(|field| {
            field.key_expr.is_none()
                && matches!(field.ty, Type::Array(_))
                && matches!(field.init, Some(Expr::Array(_) | Expr::ArraySpread(_)))
        })
        .map(|field| (field.name.as_str(), &field.ty))
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }

    let mut reads = HashSet::new();
    let mut unsafe_rebind = false;
    super::scalar_method_dispatch::for_each_expr_in_stmts(&method.body, &mut |expr| {
        match expr {
            Expr::PropertyGet {
                object, property, ..
            } if matches!(object.as_ref(), Expr::This)
                && candidates.contains_key(property.as_str()) =>
            {
                reads.insert(property.clone());
            }
            Expr::PropertySet {
                object, property, ..
            }
            | Expr::PropertyUpdate {
                object, property, ..
            } if matches!(object.as_ref(), Expr::This)
                && candidates.contains_key(property.as_str()) =>
            {
                unsafe_rebind = true;
            }
            // A computed own-property write could name any candidate field.
            Expr::IndexSet { object, .. } | Expr::IndexUpdate { object, .. }
                if matches!(object.as_ref(), Expr::This) =>
            {
                unsafe_rebind = true;
            }
            Expr::PutValueSet {
                target, receiver, ..
            } if matches!(target.as_ref(), Expr::This)
                || matches!(receiver.as_ref(), Expr::This) =>
            {
                unsafe_rebind = true;
            }
            Expr::Delete(inner)
                if matches!(
                    inner.as_ref(),
                    Expr::PropertyGet { object, .. } | Expr::IndexGet { object, .. }
                        if matches!(object.as_ref(), Expr::This)
                ) =>
            {
                unsafe_rebind = true;
            }
            // `this.m()` is safe for the shape proof because the callee is
            // vetted transitively, but its stores could replace a cached
            // array slot. Keep this local transform independent of that
            // transitive analysis and decline the clone.
            Expr::Call { callee, .. }
                if matches!(
                    callee.as_ref(),
                    Expr::PropertyGet { object, .. }
                        if matches!(object.as_ref(), Expr::This)
                ) =>
            {
                unsafe_rebind = true;
            }
            Expr::SuperPropertySet { .. }
            | Expr::SuperMethodCall { .. }
            | Expr::SuperMethodCallSpread { .. } => {
                unsafe_rebind = true;
            }
            _ => {}
        }
    });
    if unsafe_rebind {
        return Vec::new();
    }

    class
        .fields
        .iter()
        .filter(|field| reads.contains(field.name.as_str()))
        .filter_map(|field| {
            candidates
                .get(field.name.as_str())
                .map(|ty| (field.name.clone(), (*ty).clone()))
        })
        .collect()
}

fn stmts_contain_loop(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|stmt| match stmt {
        Stmt::While { .. } | Stmt::DoWhile { .. } | Stmt::For { .. } => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmts_contain_loop(then_branch)
                || else_branch.as_deref().is_some_and(stmts_contain_loop)
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            stmts_contain_loop(body)
                || catch
                    .as_ref()
                    .is_some_and(|catch| stmts_contain_loop(&catch.body))
                || finally.as_deref().is_some_and(stmts_contain_loop)
        }
        Stmt::Switch { cases, .. } => cases.iter().any(|case| stmts_contain_loop(&case.body)),
        Stmt::Labeled { body, .. } => stmts_contain_loop(std::slice::from_ref(body.as_ref())),
        _ => false,
    })
}

fn rewrite_array_field_reads_in_stmts(stmts: &mut [Stmt], aliases: &HashMap<String, (u32, Type)>) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(init) = init {
                    rewrite_array_field_reads(init, aliases);
                }
            }
            Stmt::Expr(expr) | Stmt::Throw(expr) => {
                rewrite_array_field_reads(expr, aliases);
            }
            Stmt::Return(expr) => {
                if let Some(expr) = expr {
                    rewrite_array_field_reads(expr, aliases);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                rewrite_array_field_reads(condition, aliases);
                rewrite_array_field_reads_in_stmts(then_branch, aliases);
                if let Some(else_branch) = else_branch {
                    rewrite_array_field_reads_in_stmts(else_branch, aliases);
                }
            }
            Stmt::While { condition, body } => {
                rewrite_array_field_reads(condition, aliases);
                rewrite_array_field_reads_in_stmts(body, aliases);
            }
            Stmt::DoWhile { body, condition } => {
                rewrite_array_field_reads_in_stmts(body, aliases);
                rewrite_array_field_reads(condition, aliases);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    rewrite_array_field_reads_in_stmts(
                        std::slice::from_mut(init.as_mut()),
                        aliases,
                    );
                }
                if let Some(condition) = condition {
                    rewrite_array_field_reads(condition, aliases);
                }
                if let Some(update) = update {
                    rewrite_array_field_reads(update, aliases);
                }
                rewrite_array_field_reads_in_stmts(body, aliases);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                rewrite_array_field_reads_in_stmts(body, aliases);
                if let Some(catch) = catch {
                    rewrite_array_field_reads_in_stmts(&mut catch.body, aliases);
                }
                if let Some(finally) = finally {
                    rewrite_array_field_reads_in_stmts(finally, aliases);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                rewrite_array_field_reads(discriminant, aliases);
                for case in cases {
                    if let Some(test) = &mut case.test {
                        rewrite_array_field_reads(test, aliases);
                    }
                    rewrite_array_field_reads_in_stmts(&mut case.body, aliases);
                }
            }
            Stmt::Labeled { body, .. } => {
                rewrite_array_field_reads_in_stmts(std::slice::from_mut(body.as_mut()), aliases)
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

fn rewrite_array_field_reads(expr: &mut Expr, aliases: &HashMap<String, (u32, Type)>) {
    if let Expr::PropertyGet {
        object, property, ..
    } = expr
    {
        if matches!(object.as_ref(), Expr::This) {
            if let Some((id, _)) = aliases.get(property) {
                *expr = Expr::LocalGet(*id);
                return;
            }
        }
    }
    // A nested ordinary function has its own dynamic `this`. A lexical arrow
    // that mentioned the method receiver was already rejected by
    // `method_proven_this`, so no eligible reference is lost here.
    if matches!(expr, Expr::Closure { .. }) {
        return;
    }
    perry_hir::walker::walk_expr_children_mut(expr, &mut |child| {
        rewrite_array_field_reads(child, aliases)
    });
}

/// Drop any proven-`this` clone whose method pair never made it into the
/// method registry: a pair with no registered public symbol could never have
/// been emitted, and a routing site consulting `pshape_methods` must never
/// route to a clone the emission loop cannot produce.
///
/// Until #6927's reserved-`$` namespace this also pruned composed-symbol
/// collisions with user members literally named `{method}__pshape`; that arm
/// is now dead by construction (no registry symbol can contain `$`, so no
/// registry symbol can equal `pshape_method_name(public)`) and was deleted.
pub(crate) fn prune_unregistered_clones(
    pshape_methods: &mut HashMap<(String, String), PtrShapeLocal>,
    method_names: &HashMap<(String, String), String>,
) {
    if pshape_methods.is_empty() {
        return;
    }
    pshape_methods.retain(|key, _| method_names.contains_key(key));
}

/// The `Object.freeze` / `Object.seal` / `Object.preventExtensions` family.
///
/// Deliberately NOT part of [`super::ptr_shape::expr_is_shape_barrier`] — see
/// `ModuleDispatchFacts::freeze_barrier_sites` for why a Phase 3b local needs
/// no module-wide kill here but a proven `this` does.
pub(crate) fn expr_is_freeze_barrier(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::ObjectFreeze(_) | Expr::ObjectSeal(_) | Expr::ObjectPreventExtensions(_)
    )
}

/// Admission test for one instance method. `Some(fact)` means a
/// `{public}$pshape` clone may be emitted, with `fact` installed as
/// `FnCtx::proven_this`.
pub(crate) fn method_proven_this(
    class: &Class,
    method: &Function,
    classes: &HashMap<String, &Class>,
    module_dispatch: &ModuleDispatchFacts,
) -> Option<PtrShapeLocal> {
    if !ptr_shape_this_enabled() || !ptr_shape_locals_enabled() {
        return None;
    }
    // The module-wide §5.2 barrier kill, shared verbatim with Phase 3b.
    if module_dispatch.has_shape_barrier_sites() {
        return None;
    }
    // Context restrictions, same as Phase 1/3a/3b: the async-to-generator
    // transform boxes body locals into a shared cell, so `this`-flow facts do
    // not survive it.
    if method.is_async || method.is_generator || method.was_plain_async {
        return None;
    }
    if !method.captures.is_empty() {
        return None;
    }
    for param in &method.params {
        if param.default.is_some() || param.is_rest || param.arguments_object.is_some() {
            return None;
        }
    }
    // Class-level admission (accessor-free, computed-free, statically-extended,
    // modeled chain) and method-table stability.
    if !chain_admissible(classes, &class.name) {
        return None;
    }
    if !module_dispatch.prototype_is_stable(classes, &class.name) {
        return None;
    }

    let chain = chain_classes(classes, &class.name);
    if chain.is_empty() {
        return None;
    }
    let fields = chain_field_names(&chain);
    let methods = chain_method_map(&chain);

    // `this`-flow safety: `this` never used as a value (`Expr::This` in value
    // position rejects), no closure mentioning `this`, every `this.f = v`
    // write to a DECLARED chain field, every internally-invoked `this.m()` /
    // `super.m()` vetted transitively. This is the same walk Phase 3b runs
    // over the methods called on a proven local.
    let mut analysis = ThisFlowAnalysis::new(&chain, &fields, &methods);
    if !analysis.method_safe(&class.name, method) {
        return None;
    }

    // Does the clone (including everything it transitively invokes on the same
    // `this`) contain a field WRITE? A guard-free raw store into a frozen or
    // sealed receiver would silently succeed where the spec requires a
    // strict-mode TypeError, and unlike a Phase 3b local the receiver here is
    // aliased by construction. Reads are unaffected — a frozen object still
    // reads back exactly the same slot bits.
    let writes_fields = analysis.has_this_store_records();
    if writes_fields && module_dispatch.has_freeze_barrier_sites() {
        return None;
    }

    // The clone must actually remove work: a method that never touches a
    // declared field through `this` lowers identically to the public body and
    // would be pure code-size bloat. Writes are counted through the store
    // records (which cover every HIR store form — `PropertySet`,
    // `PropertyUpdate`, `PutValueSet`), reads through the body walk.
    if !writes_fields && !method_reads_chain_field(method, &fields) {
        return None;
    }

    Some(PtrShapeLocal {
        class_name: class.name.clone(),
        // See the module doc: never claimed for a proven `this`.
        numeric_fields: HashSet::new(),
        // Phase 5a's promoted value is the receiver, not a named binding.
        report_name: crate::opt_report::enabled().then(|| String::from("this")),
    })
}

/// Producer-side capabilities that may be published to native-module
/// consumers.
///
/// This deliberately uses only classes defined in `hir`. A class whose parent
/// is imported therefore stays out of the published set even when the full
/// compile options later make its chain resolvable. Under-publishing only
/// leaves a guarded call on the public body; over-publishing could make a
/// consumer reference a clone the producer did not emit.
pub(crate) fn exportable_method_capabilities(
    hir: &perry_hir::Module,
) -> (HashMap<String, Vec<String>>, HashMap<String, Vec<String>>) {
    let classes: HashMap<String, &Class> = hir
        .classes
        .iter()
        .map(|class| (class.name.clone(), class))
        .collect();
    let module_dispatch = super::collect_module_dispatch_facts(hir);
    let mut exported = HashMap::new();
    let mut tower_routable = HashMap::new();
    let mut eligible = HashSet::new();

    for class in &hir.classes {
        let mut methods = Vec::new();
        for method in &class.methods {
            if method_proven_this(class, method, &classes, &module_dispatch).is_none() {
                continue;
            }
            methods.push(method.name.clone());
            eligible.insert((class.name.clone(), method.name.clone()));
        }
        if !methods.is_empty() {
            exported.insert(class.name.clone(), methods);
        }
    }

    // Price tower routes only after every clone capability is known: a clone
    // can delete the public guard at a nested `this.other()` boundary too,
    // including when `other` is inherited from another class in the chain.
    for class in &hir.classes {
        let tower_methods: Vec<String> = class
            .methods
            .iter()
            .filter(|method| eligible.contains(&(class.name.clone(), method.name.clone())))
            .filter(|method| tower_route_profitable(class, method, &classes, &eligible))
            .map(|method| method.name.clone())
            .collect();
        if !tower_methods.is_empty() {
            tower_routable.insert(class.name.clone(), tower_methods);
        }
    }

    (exported, tower_routable)
}

/// #7142 profitability: should a class-id dispatch-tower case route to
/// `method`'s `{public}$pshape` clone?
///
/// Only meaningful once [`method_proven_this`] has admitted a clone — this adds
/// the "should we?" half that the admission test (a pure "may we?" conjunction)
/// deliberately does not carry. Refusing is always sound: the tower case keeps
/// calling the public body.
///
/// The two other routing sites do NOT consult this. They are dominated by a
/// shape guard that is paid whether or not the clone is taken, so for them the
/// clone is free; only the tower pays for its own proof.
pub(crate) fn tower_route_profitable(
    class: &Class,
    method: &Function,
    classes: &HashMap<String, &Class>,
    eligible: &HashSet<(String, String)>,
) -> bool {
    let chain = chain_classes(classes, &class.name);
    if chain.is_empty() {
        return false;
    }
    let fields = chain_field_names(&chain);
    let pshape_methods = chain_method_map(&chain)
        .into_iter()
        .filter_map(|(name, (owner, _))| eligible.contains(&(owner, name.clone())).then_some(name))
        .collect();
    super::repsel_benefit::tower_route_profitable(method, &fields, &pshape_methods)
}

/// Does the method body READ `this.<declared chain field>` anywhere?
fn method_reads_chain_field(method: &Function, fields: &HashSet<String>) -> bool {
    let mut found = false;
    super::scalar_method_dispatch::for_each_expr_in_stmts(&method.body, &mut |e| {
        if found {
            return;
        }
        if let Expr::PropertyGet {
            object, property, ..
        } = e
        {
            if matches!(object.as_ref(), Expr::This) && fields.contains(property.as_str()) {
                found = true;
            }
        }
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeze_family_is_a_barrier() {
        assert!(expr_is_freeze_barrier(&Expr::ObjectFreeze(Box::new(
            Expr::This
        ))));
        assert!(expr_is_freeze_barrier(&Expr::ObjectSeal(Box::new(
            Expr::This
        ))));
        assert!(expr_is_freeze_barrier(&Expr::ObjectPreventExtensions(
            Box::new(Expr::This)
        )));
        // Not a freeze barrier: the Phase 3b §5.2 family is tracked separately.
        assert!(!expr_is_freeze_barrier(&Expr::Delete(Box::new(Expr::This))));
        assert!(!expr_is_freeze_barrier(&Expr::This));
    }

    /// Issue #6927: the clone symbol lives in the reserved `$` namespace, so
    /// no user member can forge it — a class with methods `foo` and
    /// `foo__pshape` (or `foo$pshape`, or a cross-class shape like class
    /// `C__foo` + method `pshape`) composes publics that can never equal
    /// `foo`'s clone symbol, and BOTH keep their clones.
    #[test]
    fn clone_symbols_are_unforgeable_and_unregistered_pairs_prune() {
        let fact = || PtrShapeLocal {
            class_name: "C".to_string(),
            numeric_fields: HashSet::new(),
            report_name: None,
        };
        let k = |m: &str| ("C".to_string(), m.to_string());

        // The old forgery shapes: same-class `foo__pshape`, cross-class
        // `C__foo` + `pshape`. Every registry symbol is sanitize-produced
        // (`[A-Za-z0-9_]` only) and therefore `$`-free; the clone symbol
        // always contains `$`.
        let mut method_names = HashMap::new();
        method_names.insert(k("foo"), "perry_method_m__C__foo".to_string());
        method_names.insert(
            k("foo__pshape"),
            "perry_method_m__C__foo__pshape".to_string(),
        );
        method_names.insert(
            ("C__foo".to_string(), "pshape".to_string()),
            "perry_method_m__C__foo__pshape__dup1".to_string(),
        );
        for public in method_names.values() {
            assert!(
                !public.contains('$'),
                "registry symbols are sanitize-produced and must never \
                 contain `$`: {public}"
            );
        }
        let clone = pshape_method_name(&method_names[&k("foo")]);
        assert_eq!(clone, "perry_method_m__C__foo$pshape");
        assert!(
            !method_names.values().any(|public| *public == clone),
            "no registered public symbol can equal a clone symbol"
        );

        // Both the promoted method AND its forgery-named sibling keep their
        // clones now — nothing stands down.
        let mut pshape = HashMap::new();
        pshape.insert(k("foo"), fact());
        pshape.insert(k("foo__pshape"), fact());
        prune_unregistered_clones(&mut pshape, &method_names);
        assert!(pshape.contains_key(&k("foo")));
        assert!(pshape.contains_key(&k("foo__pshape")));

        // A pair not present in the registry can never have been emitted.
        let mut pshape = HashMap::new();
        pshape.insert(k("ghost"), fact());
        prune_unregistered_clones(&mut pshape, &HashMap::new());
        assert!(pshape.is_empty());
    }

    /// The proven-`this` clone suffix must never be registered into a runtime
    /// vtable or reachable from any indirect route. This is the Phase 5a twin
    /// of `spec_abi_symbol_reachability`; it is deliberately a SEPARATE
    /// ratchet rather than a widening of that test's allowlist, so the Phase 2
    /// guarantee stays exactly as tight as it was.
    #[test]
    fn pshape_symbol_reachability() {
        let src_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        // Naming + emission + the two proven call sites. `string_pool.rs`
        // (which emits `js_register_class_method`) is deliberately ABSENT:
        // the vtable must only ever hold the public symbol.
        let allowed: [&str; 8] = [
            "collectors/proven_this.rs",                   // this test
            "collectors/proven_this_routing_tests.rs",     // routing IR ratchet
            "codegen/guarded_undefined_method_tests.rs",   // test-only wrapper linkage ratchet
            "codegen/typed_abi.rs",                        // name helper
            "codegen/method.rs",                           // clone emission
            "codegen/artifacts.rs",                        // emission driver
            "lower_call/method_override.rs",               // guarded fast-arm routing
            "lower_call/property_get/dynamic_dispatch.rs", // guard-free routing
        ];
        let mut offenders: Vec<String> = Vec::new();
        fn visit(
            dir: &std::path::Path,
            root: &std::path::Path,
            allowed: &[&str],
            out: &mut Vec<String>,
        ) {
            for entry in std::fs::read_dir(dir).expect("read src dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    visit(&path, root, allowed, out);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if allowed.contains(&rel.as_str()) {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("read source file");
                if text.contains("$pshape") {
                    out.push(rel);
                }
            }
        }
        visit(&src_root, &src_root, &allowed, &mut offenders);
        assert!(
            offenders.is_empty(),
            "proven-`this` clone symbol fragments found outside the allowlist \
             (the clone must NEVER be registered into a \
             runtime vtable or reached indirectly): {offenders:?}"
        );
    }
}
