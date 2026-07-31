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
//! Phase 5a emits an `internal` `{public}__pshape` clone of the method whose
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
//! Gated by `PERRY_PTR_SHAPE_THIS` (default on; `0`/`off`/`false` disables —
//! keyed into the object cache). Also honours `PERRY_PTR_SHAPE_LOCALS`, since
//! Phase 5a is an extension of the same `Ptr<Shape>` proof.

use std::collections::{HashMap, HashSet};

use perry_hir::{Class, Expr, Function};

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
/// This symbol is NEVER registered into a runtime vtable
/// (`js_register_class_method` keeps the public name) and is reachable only
/// from the two proven call sites. [`tests::pshape_symbol_reachability`]
/// ratchets that.
pub(crate) fn pshape_method_name(public_name: &str) -> String {
    format!("{public_name}__pshape")
}

/// Drop any proven-`this` clone whose composed symbol would collide with a
/// symbol the module already defines for a real, user-declared member.
///
/// `pshape_method_name` builds the clone symbol by appending a suffix to the
/// public one, so a class with methods `foo` and `foo__pshape` would have
/// `foo`'s clone and `foo__pshape`'s PUBLIC entry resolve to the same LLVM
/// symbol — two definitions of one name. (Cross-class shapes collide the same
/// way, e.g. a class literally named `C__foo` with a method `pshape`, because
/// the `__` separators are not escaped either — which is why this compares
/// composed SYMBOLS from the registry rather than member names.)
///
/// This is a **local mitigation for this phase's suffix only**. The hazard is
/// pre-existing and shared by the whole generated-clone family (`__generic`,
/// `__typed_f64`, `__typed_i32`, `__typed_i1`, `__typed_string`,
/// `__typed_f64_recv`, the Phase 2 specialized-ABI suffix, `__pshape`) —
/// `foo` + `foo__generic`
/// already collides today. The family-wide fix (a reserved escape in
/// `sanitize_member`, content-addressed mangling, or a uniquifier) is tracked
/// in **issue #6927**; it is deliberately not attempted here because it would
/// touch every clone kind plus Phase 2's specialized-ABI reachability ratchet
/// (whose allowlist must stay exactly as tight as it is — this comment
/// deliberately avoids naming that suffix literally, because the ratchet
/// scans for it).
///
/// Standing down is always sound: without a clone, both routing sites fall
/// through to today's guarded lowering, which is correct for any receiver.
pub(crate) fn prune_colliding_clones(
    pshape_methods: &mut HashMap<(String, String), PtrShapeLocal>,
    method_names: &HashMap<(String, String), String>,
) {
    if pshape_methods.is_empty() {
        return;
    }
    let taken: HashSet<&str> = method_names.values().map(String::as_str).collect();
    pshape_methods.retain(|key, _| match method_names.get(key) {
        // Keep only when the clone symbol is not already a defined symbol.
        Some(public) => !taken.contains(pshape_method_name(public).as_str()),
        // Not in the registry at all: it could never have been emitted.
        None => false,
    });
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
/// `{public}__pshape` clone may be emitted, with `fact` installed as
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

    /// Issue #6927: a class with methods `foo` and `foo__pshape` would give
    /// `foo`'s clone the same LLVM symbol as `foo__pshape`'s PUBLIC entry.
    /// The colliding clone must stand down (falling back to the always-correct
    /// guarded lowering); the innocent one alongside it must survive.
    #[test]
    fn colliding_clone_symbols_are_pruned() {
        let fact = || PtrShapeLocal {
            class_name: "C".to_string(),
            numeric_fields: HashSet::new(),
            report_name: None,
        };
        let k = |m: &str| ("C".to_string(), m.to_string());
        let mut method_names = HashMap::new();
        method_names.insert(k("foo"), "perry_method_m__C__foo".to_string());
        // A real user member whose symbol IS `foo`'s clone symbol.
        method_names.insert(
            k("foo__pshape"),
            "perry_method_m__C__foo__pshape".to_string(),
        );
        method_names.insert(k("safe"), "perry_method_m__C__safe".to_string());

        let mut pshape = HashMap::new();
        pshape.insert(k("foo"), fact());
        pshape.insert(k("safe"), fact());
        prune_colliding_clones(&mut pshape, &method_names);

        assert!(
            !pshape.contains_key(&k("foo")),
            "`foo`'s clone collides with `foo__pshape`'s public symbol and must be dropped"
        );
        assert!(
            pshape.contains_key(&k("safe")),
            "a non-colliding clone must survive the prune"
        );

        // Cross-CLASS collision: class `C__foo` + method `pshape` composes the
        // exact same symbol, because `__` separators are not escaped either.
        let mut method_names = HashMap::new();
        method_names.insert(k("foo"), "perry_method_m__C__foo".to_string());
        method_names.insert(
            ("C__foo".to_string(), "pshape".to_string()),
            "perry_method_m__C__foo__pshape".to_string(),
        );
        let mut pshape = HashMap::new();
        pshape.insert(k("foo"), fact());
        prune_colliding_clones(&mut pshape, &method_names);
        assert!(
            pshape.is_empty(),
            "cross-class symbol collisions must be caught too — the check \
             compares composed SYMBOLS, not member names"
        );

        // A pair not present in the registry can never have been emitted.
        let mut pshape = HashMap::new();
        pshape.insert(k("ghost"), fact());
        prune_colliding_clones(&mut pshape, &HashMap::new());
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
        let allowed: [&str; 7] = [
            "collectors/proven_this.rs",                   // this test
            "collectors/proven_this_routing_tests.rs",     // routing IR ratchet
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
                if text.contains("__pshape") {
                    out.push(rel);
                }
            }
        }
        visit(&src_root, &src_root, &allowed, &mut offenders);
        assert!(
            offenders.is_empty(),
            "proven-`this` clone symbol fragments found outside the allowlist \
             (the clone is `internal` and must NEVER be registered into a \
             runtime vtable or reached indirectly): {offenders:?}"
        );
    }
}
