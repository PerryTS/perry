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
        // `param_type_guard.rs`: read-only descriptor/heap traversal. It may
        // use Rust Vec/TLS registries, but never allocates in Perry's heap or
        // invokes JavaScript getters, proxies, coercions, or callbacks.
        | "js_param_type_guard"
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
        | "js_gc_declare_typed_shape_layout"
        // #7834: `layout_forget_object` behind a null check — two thread-local
        // side-table removals, no allocation and no re-entry.
        | "js_gc_forget_object_layout"
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
        // #8596: closure-call IC guard. Its body reads the closure header and
        // does registry lookups (arity/rest/func-ptr — all reads), builds a
        // stack `Observation`, and calls `guard_observe` (`typed_feedback.rs`),
        // whose whole effect is a TLS registry borrow, saturating counters, and
        // a `Vec::push` of the observation — a raw Rust allocation, which by the
        // documented rule above cannot arm a Perry-GC trigger. No Perry
        // allocation, no re-entry into generated JS, no throw. Same class as the
        // two observe guards above; unlike the excluded get/set wrappers it does
        // NOT perform an object get/set. In `NONCOLLECTING` (the audit authority).
        | "js_typed_feedback_closure_direct_call_guard"
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
        // #7469: two header-bit writes plus the same `layout_forget_object`
        // side-table remove `layout_init_pointer_free` already does on every
        // allocation. No Perry allocation, no re-entry into generated code.
        | "js_array_declare_all_pointer_elements"
        // TLS dynamic-call context only. #8596 adds the `_get` reader — a bare
        // `IMPLICIT_THIS.with(|c| f64::from_bits(c.get()))` (`object/this_binding.rs`),
        // the exact shape of the already-admitted `_set` and `js_new_target_get`.
        // The `_sloppy` reader is deliberately ABSENT: it boxes booleans/strings
        // and reads globalThis, which allocates.
        | "js_implicit_this_set"
        | "js_implicit_this_get"
        | "js_new_target_get"
        | "js_new_target_set"
        // #8596: TDZ-suppression window depth (`box.rs`). Each is a single
        // thread-local `Cell<u32>` saturating inc/dec, emitted in pairs around a
        // side-effect-free class-capture snapshot. No allocation, no re-entry.
        | "js_tdz_suppress_begin"
        | "js_tdz_suppress_end"
        // Closure capture-slot accessors (#8132). `closure/alloc.rs`:
        // `get` is a null check, a bounds check, and a raw slot read;
        // `set` is the raw slot write plus `note_closure_capture_slot`, whose
        // whole body is `layout_note_slot` + `runtime_write_barrier_gc_slot` —
        // the same side-table/barrier bodies already admitted above as
        // `js_gc_note_slot_layout` / `js_write_barrier_slot`. The `_ptr`
        // spellings are one-line wrappers over the `_bits` pair. All four are
        // in `gc_root_dominance_check.py`'s NONCOLLECTING (the audit
        // authority this table must stay a subset of). On #8132's bundled
        // module factory these were 1,495 of 5,537 statepoints.
        | "js_closure_get_capture_bits"
        | "js_closure_set_capture_bits"
        | "js_closure_set_box_capture_ptr"
        | "js_closure_get_capture_ptr"
        | "js_closure_set_capture_ptr"
        // Variable-box accessors and allocators (#8132), `box.rs`. Boxes are
        // `std::alloc::alloc` allocations OUTSIDE the GC heap — allocating
        // one arms no Perry GC trigger (the malloc-count trigger counts
        // `MALLOC_STATE` GC objects, not raw Rust allocations), and the
        // registry insert is a TLS set. `gc_root_dominance_check.py`'s
        // IMMOVABLE_SOURCES "box" probes pin exactly this: std::alloc, no
        // arena allocation, no dealloc — if boxes ever become GC objects the
        // lint fails and these entries must be demoted with it. The setters
        // are a registry membership check, the raw cell write, and (for the
        // JSValue box) `runtime_write_barrier_root_nanbox`, admitted above.
        // The i32/bool getters are registry check + raw read; they have no
        // TDZ path.
        //
        // `js_box_get_bits` is deliberately ABSENT: its TDZ arm calls
        // `js_throw_reference_error_tdz`, which allocates the ReferenceError
        // (string + error object) before unwinding — a genuine route into
        // collection, per the `js_throw*` audit note below. The checker's
        // NONCOLLECTING currently lists it anyway; this table does not
        // inherit that entry, it only requires containment in the safe
        // direction.
        | "js_box_alloc_bits"
        | "js_i32_box_alloc"
        | "js_bool_box_alloc"
        | "js_box_set_bits"
        | "js_box_set_bits_trusted_no_barrier"
        | "js_i32_box_set"
        | "js_bool_box_set"
        | "js_i32_box_get"
        | "js_bool_box_get"
        // The #7933 release entry points: registry remove + raw cell clear +
        // TLS free-pool push. No GC-heap allocation, no user code, no
        // collection trigger — the same audit as the accessors above.
        | "js_box_release"
        | "js_i32_box_release"
        | "js_bool_box_release" => GcCallEffect::CannotCollect,
        // Audited allocate-but-never-reenter helpers (2026-07-31): each body
        // was checked for closure invocation, coercion (valueOf/toString),
        // and accessor dispatch — none present, and none takes a receiver
        // that could route through user code (`js_array_length` takes a
        // typed `*const ArrayHeader`, not a JSValue). The forced-evacuation
        // probe gates backstop the audit.
        "js_closure_alloc_singleton"
        | "js_object_alloc_class_inline_keys"
        | "js_object_alloc_class_inline_keys_stamped"
        | "js_array_push_f64"
        | "js_array_length"
        | "js_array_slice_values"
        // Second audit round (2026-08-01): ctor-return semantics check
        // (inspects the returned value, calls nothing), strict-equality
        // indexOf scan (strict equality never runs user code), and the two
        // callback-type validators (type check + static-message throw; their
        // throw path is the audited noreturn funnel). Deliberately NOT
        // admitted: js_value_length_f64 — its plain-object arm calls
        // js_object_get_field_by_name_f64, a transitive getter path;
        // js_value_length_property_f64 deliberately delegates the full
        // property/getter path; and js_array_get_f64 has hole/accessor paths.
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

    /// The box/closure family's stated containment in the checker's
    /// `NONCOLLECTING` is now CHECKED rather than merely asserted in prose.
    ///
    /// Three comments above claim membership in `NONCOLLECTING`
    /// (`scripts/gc_root_dominance_check.py`) for the closure capture-slot
    /// accessors, the box accessors/allocators, and — since #8208 — the box
    /// release entry points, naming it "the audit authority this table must
    /// stay a subset of". Nothing enforced it. #7510 is what one-sided drift
    /// costs: the two lists disagreed and the checker printed 358 spurious
    /// violations once the corpus widened. #8208 drifted them again by adding
    /// the three `js_*box_release` names here and not there; a human reading
    /// the diff caught it, which is not a gate.
    ///
    /// SCOPE, deliberately narrow. A whole-table subset is NOT asserted,
    /// because it is not true: 28 entries here (the `js_typed_*` guards, the
    /// feedback counters, `js_param_type_guard`, `js_nanbox_pointer`,
    /// `js_string_addref`, …) are absent from `NONCOLLECTING` today. That
    /// divergence is safe in the direction it runs — the checker's
    /// `is_collecting` treats an unknown callee as collecting, so a missing
    /// entry costs a false POSITIVE, which the script's own header calls its
    /// one-sided design. Adding those 28 names would make the checker LESS
    /// conservative and could hide real violations, so each needs its own
    /// audit evidence; that is a separate decision, not a tidy-up to be
    /// smuggled in here. What this test pins is the family whose containment
    /// the comments actually claim, which is also exactly where this PR
    /// drifted.
    #[test]
    fn box_and_closure_helpers_stay_contained_in_the_checker_authority() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let rust_src = std::fs::read_to_string(format!("{manifest}/src/gc_call_effects.rs"))
            .expect("read gc_call_effects.rs");
        let py_path = format!("{manifest}/../../scripts/gc_root_dominance_check.py");
        let py_src = std::fs::read_to_string(&py_path).expect("read gc_root_dominance_check.py");

        let noncollecting = extract_python_set(&py_src, "NONCOLLECTING");
        assert!(
            noncollecting.len() > 50,
            "parsed only {} NONCOLLECTING names — the extraction broke, and a \
             vacuous containment check is worse than none",
            noncollecting.len()
        );

        let cannot_collect = extract_cannot_collect_arms(&rust_src);
        // The subject must be live: if the extraction silently stopped seeing
        // the family, the containment loop below would pass by checking
        // nothing at all.
        let family: Vec<&str> = cannot_collect
            .iter()
            .filter(|n| n.contains("_box_") || n.contains("_closure_"))
            .copied()
            .collect();
        assert!(
            family.len() >= 12,
            "parsed only {} box/closure helpers from the CannotCollect arms \
             ({family:?}) — extraction broke",
            family.len()
        );
        for probe in [
            "js_box_release",
            "js_i32_box_release",
            "js_bool_box_release",
            "js_box_alloc_bits",
            "js_closure_get_capture_bits",
        ] {
            assert!(
                family.contains(&probe),
                "{probe} missing from the parsed box/closure family"
            );
        }

        let missing: Vec<&str> = family
            .iter()
            .filter(|n| !noncollecting.contains(**n))
            .copied()
            .collect();
        assert!(
            missing.is_empty(),
            "these box/closure callees are CannotCollect here but absent from \
             NONCOLLECTING in scripts/gc_root_dominance_check.py, which the \
             comments above name as the audit authority this family must stay \
             contained in: {missing:?}"
        );
    }

    /// Collect the string literals of a top-level `NAME = {...}` python set.
    fn extract_python_set<'a>(src: &'a str, name: &str) -> std::collections::HashSet<&'a str> {
        let mut out = std::collections::HashSet::new();
        let mut inside = false;
        for line in src.lines() {
            if !inside {
                if line.starts_with(&format!("{name} = {{")) {
                    inside = true;
                }
                continue;
            }
            if line == "}" {
                break;
            }
            let code = line.split('#').next().unwrap_or("");
            out.extend(quoted_literals(code));
        }
        out
    }

    /// Collect the callee names of every match arm resolving to
    /// `GcCallEffect::CannotCollect`. Arms are `| "name"` chains, freely
    /// interleaved with `//` comments, terminated by the `=>`.
    fn extract_cannot_collect_arms(src: &str) -> std::collections::HashSet<&str> {
        let mut out = std::collections::HashSet::new();
        let mut pending: Vec<&str> = Vec::new();
        for line in src.lines() {
            let t = line.trim();
            if t.starts_with("//") || t.is_empty() {
                continue;
            }
            // Only accumulate from pure pattern lines: `| "a"` / `"a" | "b"`.
            let is_pattern = (t.starts_with('|') || t.starts_with('"'))
                && !t.contains("=>")
                && !t.contains("assert");
            if is_pattern {
                pending.extend(quoted_literals(t));
                continue;
            }
            if let Some(head) = t.split("=>").next() {
                if t.contains("=>") {
                    let mut names = pending.clone();
                    names.extend(quoted_literals(head));
                    if t.contains("GcCallEffect::CannotCollect") {
                        out.extend(names);
                    }
                    pending.clear();
                    continue;
                }
            }
            pending.clear();
        }
        out
    }

    fn quoted_literals(s: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'"' {
                if let Some(end) = s[i + 1..].find('"') {
                    let lit = &s[i + 1..i + 1 + end];
                    if lit.starts_with("js_") || lit.starts_with("llvm.") {
                        out.push(lit);
                    }
                    i = i + 1 + end + 1;
                    continue;
                }
                break;
            }
            i += 1;
        }
        out
    }

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

    /// #8132: capture-slot and variable-box accessors are leaf calls. On the
    /// bundled-module-factory shape these were ~45% of all statepoints, each
    /// paying a relocation for every live GC value.
    #[test]
    fn capture_and_box_accessors_cannot_collect() {
        for name in [
            "js_closure_get_capture_bits",
            "js_closure_set_capture_bits",
            "js_closure_set_box_capture_ptr",
            "js_closure_get_capture_ptr",
            "js_closure_set_capture_ptr",
            "js_box_alloc_bits",
            "js_i32_box_alloc",
            "js_bool_box_alloc",
            "js_box_set_bits",
            "js_box_set_bits_trusted_no_barrier",
            "js_i32_box_set",
            "js_bool_box_set",
            "js_i32_box_get",
            "js_bool_box_get",
            "js_box_release",
            "js_i32_box_release",
            "js_bool_box_release",
        ] {
            assert_eq!(
                classify_direct_callee(name),
                GcCallEffect::CannotCollect,
                "{name}"
            );
        }
    }

    /// #8596: helpers reclassified as non-safepoints (`CannotCollect`), each
    /// verified against its runtime body AND already present in the checker's
    /// `NONCOLLECTING` authority (so the containment test above stays green):
    ///   - `js_typed_feedback_closure_direct_call_guard` — header/registry
    ///     reads + a `guard_observe` whose only allocation is a Rust `Vec::push`
    ///     (cannot arm a Perry-GC trigger); no re-entry, no throw.
    ///   - `js_implicit_this_get` — a bare `IMPLICIT_THIS` `Cell` read, the
    ///     shape of the already-admitted `js_implicit_this_set`.
    ///   - `js_tdz_suppress_begin`/`_end` — a thread-local `Cell<u32>` inc/dec.
    #[test]
    fn issue_8596_tls_and_feedback_guards_cannot_collect() {
        for name in [
            "js_typed_feedback_closure_direct_call_guard",
            "js_implicit_this_get",
            "js_tdz_suppress_begin",
            "js_tdz_suppress_end",
        ] {
            assert_eq!(
                classify_direct_callee(name),
                GcCallEffect::CannotCollect,
                "{name}"
            );
        }
    }

    /// The discriminating negatives for #8596: near-neighbours of the four
    /// admitted above that DO allocate or throw, so they must stay
    /// `Unknown` (a safepoint). `js_implicit_this_get_sloppy` boxes
    /// booleans/strings and reads globalThis; the ordinary `_get` it wraps is
    /// the only leaf-safe half.
    #[test]
    fn issue_8596_allocating_lookalike_stays_a_safepoint() {
        assert_eq!(
            classify_direct_callee("js_implicit_this_get_sloppy"),
            GcCallEffect::Unknown,
            "the sloppy reader boxes primitives / reads globalThis and must remain a safepoint"
        );
    }

    /// The discriminating negative for the family above: `js_box_get_bits`
    /// reads a TDZ-seeded box's sentinel and calls
    /// `js_throw_reference_error_tdz`, which ALLOCATES the ReferenceError
    /// (string + error object) before unwinding. A leaf marking would leave
    /// the catch handler's relocations unrecorded on the unwind edge. If a
    /// future split gives the non-TDZ boxes their own entry point, THAT
    /// symbol can be admitted; this one cannot.
    #[test]
    fn the_tdz_capable_box_getter_stays_a_safepoint() {
        for name in ["js_box_get_bits", "js_box_get_bits_trusted"] {
            assert_eq!(
                classify_direct_callee(name),
                GcCallEffect::Unknown,
                "{name} can throw (and allocate) on the TDZ path"
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
        // Both length helpers can reach js_object_get_field_by_name_f64 for
        // plain objects; js_array_get_f64 has hole/accessor paths.
        for name in [
            "js_value_length_f64",
            "js_value_length_property_f64",
            "js_array_get_f64",
        ] {
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
