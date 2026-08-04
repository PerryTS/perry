//! Perry-GC call effects for native-stack safepoint lowering.
//!
//! This is deliberately narrower than LLVM's memory-effect attributes. A
//! helper may mutate runtime metadata, take a lock, or allocate through the
//! system allocator and still be safe to omit as a Perry GC safepoint. The
//! only question answered here is: can this call enter Perry's collector?
//!
//! Unknown is the safe default. Adding a helper to the allowlist requires
//! auditing the complete runtime call graph for `gc_check_trigger`,
//! `js_gc_collect`, `js_gc_loop_safepoint`, or another route into collection.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GcCallEffect {
    CannotCollect,
    /// May allocate — and therefore arm a GC trigger — but never runs a
    /// collection synchronously inside the call and never re-enters generated
    /// JS (no getters, setters, valueOf/toString coercion, or callbacks).
    ///
    /// Only meaningful under `PERRY_GC_SAFEPOINT_ONLY`: the runtime contract
    /// guarantees any trigger these helpers arm either defers to a declared
    /// safepoint (moving) or collects behind a forced conservative scan (the
    /// alloc-point valve), so the caller's precise frame roots are never
    /// consumed at this call site and it needs no statepoint. Without the
    /// contract these remain safepoints.
    AllocNoReentry,
    Unknown,
}

/// Classify one direct LLVM callee name, without the leading `@`.
pub(crate) fn classify_direct_callee(name: &str) -> GcCallEffect {
    match name {
        // Pure/read-only ABI helpers audited in `module::helper_decl_attrs`.
        "js_nanbox_pointer"
        | "js_nanbox_get_pointer"
        | "js_typed_f64_arg_guard"
        | "js_typed_i32_arg_guard"
        | "js_typed_i1_arg_guard"
        | "js_typed_i1_arg_to_raw"
        | "js_typed_i32_arg_to_raw"
        | "js_typed_string_arg_guard"
        | "js_is_truthy"
        | "js_typed_feedback_plain_array_index_get_guard"
        | "js_typed_feedback_numeric_array_index_get_guard"
        | "js_typed_feedback_plain_array_index_set_guard"
        | "js_typed_feedback_numeric_array_index_set_guard"
        | "js_typed_feedback_numeric_array_push_guard"
        | "js_array_numeric_value_to_raw_f64"
        // `gc/roots/temp_roots.rs`: TLS vector operations and an incremental
        // marking barrier only. They never run a Perry collection.
        | "js_gc_temp_root_push"
        | "js_gc_temp_root_get"
        | "js_gc_temp_root_set"
        | "js_gc_temp_root_truncate"
        // `gc/barrier.rs`: remembered-set / incremental-marking maintenance.
        | "js_write_barrier"
        | "js_write_barrier_slot"
        | "js_write_barrier_root_heap_word"
        | "js_write_barrier_root_nanbox"
        // `gc/roots.rs`: registers one module-level global as a root. Audited
        // 2026-08-04 and admitted because its whole body is two calls that are
        // already covered:
        //
        //   runtime_write_barrier_root_heap_word(*root)  <- `js_write_barrier_
        //       root_heap_word` immediately above is a ONE-LINE wrapper around
        //       this exact function, and is already CannotCollect. It shades
        //       one header and calls `push_mark_seed`, which is a TLS
        //       `Vec::push` (`gc/trace.rs`) — no trace, no sweep, no trigger.
        //   GLOBAL_ROOTS.with(|r| r.borrow_mut().push(root))  <- a TLS Vec.
        //
        // The `Vec::push` is the only thing worth pausing on, because CLAUDE.md
        // lists a "malloc count threshold" as a GC trigger. It does not apply:
        // that counter is `MALLOC_STATE.objects.len()`, a registry of Perry GC
        // objects, and the `#[global_allocator]` is plain mimalloc/System with
        // no GC hook. A raw Rust allocation cannot arm a trigger — which is the
        // case the module doc above already carves out.
        //
        // Worth the audit: at 148 call sites across the probe suite this is the
        // single most frequent non-leaf callee, all of it module-init code
        // registering `@perry_global_*` roots.
        | "js_gc_register_global_root"
        // `gc/layout.rs`: side-table metadata updates only.
        | "js_gc_note_slot_layout"
        | "js_gc_note_slot_layout_aware"
        | "js_gc_init_typed_shape_layout"
        | "js_gc_init_unboxed_object_layout"
        // `typed_feedback.rs`: counters/registries only. This intentionally
        // does not include feedback wrappers that perform the actual object
        // get/set operation.
        | "js_typed_feedback_record_guard_pass"
        | "js_typed_feedback_record_guard_fail"
        | "js_typed_feedback_record_fallback_call"
        | "js_typed_feedback_class_field_get_guard"
        | "js_typed_feedback_class_field_set_guard"
        | "js_typed_feedback_observe_property_get"
        | "js_typed_feedback_observe_property_set"
        // Same family, audited 2026-08-04: under `diagnostics` it reads an env
        // var, serialises the counters with serde_json and writes a file;
        // without the feature the body is empty. No Perry allocation, no
        // re-entry into generated code, no route into collection.
        | "js_typed_feedback_maybe_dump_trace"
        // Refcount writes and array-layout observations; none enters GC.
        | "js_string_addref"
        | "js_string_addref_if_heap_string"
        | "js_array_clear_numeric_layout"
        | "js_array_note_numeric_write"
        | "js_array_is_numeric_f64_layout"
        // TLS dynamic-call context only.
        | "js_implicit_this_set"
        | "js_new_target_get"
        | "js_new_target_set" => GcCallEffect::CannotCollect,
        // Audited allocate-but-never-reenter helpers (2026-07-31): each body
        // was checked for closure invocation, coercion (valueOf/toString),
        // and accessor dispatch — none present, and none takes a receiver
        // that could route through user code (`js_array_length` takes a
        // typed `*const ArrayHeader`, not a JSValue). The forced-evacuation
        // probe gates backstop the audit.
        "js_closure_alloc_singleton"
        | "js_object_alloc_class_inline_keys"
        | "js_array_push_f64"
        | "js_array_length"
        | "js_array_slice_values"
        // Second audit round (2026-08-01): ctor-return semantics check
        // (inspects the returned value, calls nothing), strict-equality
        // indexOf scan (strict equality never runs user code), and the two
        // callback-type validators (type check + static-message throw; their
        // throw path is the audited noreturn funnel). Deliberately NOT
        // admitted: js_value_length_f64 — its plain-object arm calls
        // js_object_get_field_by_name_f64, a transitive getter path; and
        // js_array_get_f64 — hole/accessor paths.
        | "js_ctor_return_override"
        | "js_array_indexOf_jsvalue"
        | "js_validate_array_comparator"
        | "js_validate_array_map_callback" => GcCallEffect::AllocNoReentry,
        // NO `js_throw*` prefix arm. It used to classify the whole family
        // a `NeverReturns` classification that suppressed the safepoint in
        // every mode — the strongest possible, and the only one that would be
        // applied by prefix rather than exact name. That variant is DELETED,
        // not merely unused: it was never constructed, so its three match arms
        // in `precise_roots.rs` were dead, and the kill-policy in CLAUDE.md
        // says an unexercised mode is a decision nobody has made.
        //
        // Two things make that unsafe. The audit it rested on is already
        // false: `js_throw_reference_error_tdz`, `js_throw_not_a_constructor`
        // and others are declared `-> f64`, not `-> !`. And since #7302 a
        // throw UNWINDS rather than longjmps, so the call site is an `invoke`
        // whose unwind edge needs relocations — while these helpers allocate
        // the Error they raise and can therefore collect. Suppressing the
        // safepoint would leave the catch handler's roots stale after a move.
        //
        // Falling through to `Unknown` costs a few statepoints and is
        // conservative in the only direction that is safe.
        _ => GcCallEffect::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `js_gc_register_global_root` is `js_write_barrier_root_heap_word` plus
    /// a TLS `Vec::push`, so the two must never be classified differently —
    /// if a future audit demotes the barrier, this catches the sibling that
    /// would otherwise keep claiming to be leaf.
    #[test]
    fn register_global_root_tracks_the_barrier_it_wraps() {
        assert_eq!(
            classify_direct_callee("js_gc_register_global_root"),
            classify_direct_callee("js_write_barrier_root_heap_word"),
            "js_gc_register_global_root's entire body is that barrier plus a \
             TLS Vec::push; they cannot have different GC effects"
        );
    }

    /// The helpers that *do* allocate must stay out of `CannotCollect`, and
    /// this pins the two that read as pure but are not.
    ///
    /// `js_nanbox_string` looks like bit manipulation and mostly is — but its
    /// null-pointer guard calls `js_string_from_bytes` to allocate an empty
    /// string rather than boxing null. At 120 call sites it is the obvious
    /// thing to reach for next; it is not admissible.
    #[test]
    fn allocating_helpers_are_not_cannot_collect() {
        for name in ["js_nanbox_string", "js_string_from_bytes", "js_array_alloc"] {
            assert_ne!(
                classify_direct_callee(name),
                GcCallEffect::CannotCollect,
                "{name} can allocate and must not be marked gc-leaf"
            );
        }
    }

    #[test]
    fn audited_runtime_bookkeeping_cannot_collect() {
        for name in [
            "js_gc_temp_root_push",
            "js_write_barrier_root_nanbox",
            "js_gc_note_slot_layout",
            "js_typed_feedback_record_guard_pass",
            "js_string_addref",
        ] {
            assert_eq!(
                classify_direct_callee(name),
                GcCallEffect::CannotCollect,
                "{name}"
            );
        }
    }

    #[test]
    fn collection_and_unknown_calls_stay_conservative() {
        for name in [
            "js_gc_collect",
            "js_gc_loop_safepoint",
            "js_alloc_object",
            "user_function",
        ] {
            assert_eq!(
                classify_direct_callee(name),
                GcCallEffect::Unknown,
                "{name}"
            );
        }
    }

    #[test]
    fn audited_alloc_helpers_are_contract_only_non_safepoints() {
        for name in [
            "js_closure_alloc_singleton",
            "js_array_push_f64",
            "js_ctor_return_override",
            "js_array_indexOf_jsvalue",
            "js_validate_array_comparator",
        ] {
            assert_eq!(
                classify_direct_callee(name),
                GcCallEffect::AllocNoReentry,
                "{name}"
            );
        }
        // Transitive re-entry paths found by the body audit must stay out:
        // js_value_length_f64 reaches js_object_get_field_by_name_f64 for
        // plain objects; js_array_get_f64 has hole/accessor paths.
        for name in ["js_value_length_f64", "js_array_get_f64"] {
            assert_eq!(
                classify_direct_callee(name),
                GcCallEffect::Unknown,
                "{name}"
            );
        }
        // Re-entering helpers must never be in the AllocNoReentry class:
        // a poll can fire inside the callback/getter with this frame
        // mid-stack, and the caller's roots must be findable.
        for name in [
            "js_array_map",
            "js_array_sort_with_comparator",
            "js_number_coerce",
            "js_dynamic_string_or_number_add",
            "js_object_get_field_by_name_f64",
        ] {
            assert_eq!(
                classify_direct_callee(name),
                GcCallEffect::Unknown,
                "{name}"
            );
        }
    }
}
