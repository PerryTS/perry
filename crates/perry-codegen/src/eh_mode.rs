//! Invoke-EH support (#7302): the nothrow-callee predicate for the
//! `invoke` conversion at the `LlBlock` call chokepoints, and the
//! render-time phi-predecessor rewrite for inline invoke block splits.
//!
//! Perry lowers `try`/`catch` to `invoke`/`landingpad` (SEH funclets on
//! windows-msvc) — see `stmt/try_stmt.rs` for the lowering and
//! `perry-runtime/src/eh.rs` for the throw side.

use std::collections::HashMap;

/// Rewrite `phi` predecessor labels after inline invoke splits (#7302).
///
/// An `invoke` emitted mid-block is followed by an inline `eh.contN:` label,
/// so the ORIGINAL block label no longer names the block that actually
/// branches onward — the last continuation segment does. Codegen captures
/// predecessor labels for phis via `ctx.block().label` at ~150 sites; rather
/// than auditing every capture, this render-time pass maps each original
/// block label to its tail segment and rewrites the phi incoming-edge labels
/// to match. Branch/switch/invoke TARGETS are untouched — they name block
/// headers, which splits never move. Sound because a block's terminator
/// always lives in its last segment (inline labels are emitted only directly
/// after an invoke, never after a terminator), so the tail segment is the
/// one true CFG predecessor for every edge the original block owned.
pub(crate) fn rewrite_phi_predecessors(ir: &str) -> String {
    let mut tail: HashMap<&str, &str> = HashMap::new();
    let mut cur: Option<&str> = None;
    for line in ir.lines() {
        if let Some(lbl) = flush_left_label(line) {
            if lbl.starts_with("eh.cont") {
                if let Some(c) = cur {
                    tail.insert(c, lbl);
                }
            } else {
                cur = Some(lbl);
            }
        }
    }
    tail.retain(|k, v| k != v);
    if tail.is_empty() {
        return ir.to_string();
    }
    let mut out = String::with_capacity(ir.len() + 64);
    for line in ir.lines() {
        if line.contains(" = phi ") {
            let mut s = line.to_string();
            for (orig, t) in &tail {
                let from = format!(", %{} ]", orig);
                if s.contains(&from) {
                    s = s.replace(&from, &format!(", %{} ]", t));
                }
            }
            out.push_str(&s);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// `name:` at column 0 with nothing after the colon — the shape of both
/// block headers and inline continuation labels in Perry's writer.
fn flush_left_label(line: &str) -> Option<&str> {
    let rest = line.strip_suffix(':')?;
    if rest.is_empty() {
        return None;
    }
    rest.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '$' | '-'))
        .then_some(rest)
}

/// May a call to `name` be emitted as a plain `call` inside a protected
/// region? True only when the callee provably cannot unwind.
///
/// A runtime symbol answers from the generated table's `throws` column
/// (`gc_call_effects::runtime_throws`, one solve over the linked archives; a
/// symbol no table proves, such as user code, counts as throwing). That column
/// is the only source of truth for runtime calls: there is no hand list of
/// names behind it. The EH bookkeeping helpers (`js_eh_try_push`, `js_try_end`,
/// ...) and the GC entry points are in the table like any other symbol and are
/// plain calls because the graph proves them `no`.
///
/// Two clauses name things that are not runtime calls and so have no row:
/// * `llvm.*` intrinsics. The ones Perry emits do not unwind (the rule
///   `callee_is_nothrow` had); slice b replaces the prefix with LLVM's own
///   intrinsic attributes.
/// * `js_shadow_*`: the root-lowering directives (`js_shadow_slot_bind`,
///   `js_shadow_frame_enter`, ...). The native-roots lowering pattern-matches
///   them as plain `call`s and erases them, so an `invoke` would leave the
///   function un-lowered; on shadow-frame targets they are stack bookkeeping
///   that cannot unwind. They are absent from the Linux table because the
///   runtime no longer exports them there.
pub(crate) fn nothrow_callee(name: &str) -> bool {
    name.starts_with("llvm.")
        || name.starts_with("js_shadow_")
        || !crate::gc_call_effects::runtime_throws(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_property_miss_is_still_throwing() {
        for name in [
            "js_object_get_field_ic_miss_packed",
            // T1: the generic-get tower's two slow exits carry the same `cold`
            // placement hint — and the same ability to throw, now including
            // the nullish-receiver TypeError the emitted arm used to raise
            // inline. A `cold` helper that lost its invoke edge would silently
            // swallow every `Cannot read properties of undefined`.
            "js_object_get_field_ic_slow",
            "js_object_get_field_ic_nonptr",
        ] {
            assert_eq!(crate::module::helper_decl_attrs(name), " cold", "{name}");
            assert!(!nothrow_callee(name), "{name}");
        }
        assert!(!nothrow_callee("js_object_get_field_ic_miss"));
        assert!(!nothrow_callee("unknown_runtime_helper"));
    }

    #[test]
    fn semantic_nounwind_groups_keep_their_contract() {
        for name in [
            "js_nanbox_pointer",
            "js_is_truthy",
            "js_string_compare",
            "js_array_numeric_value_to_raw_f64",
        ] {
            assert!(nothrow_callee(name), "{name}");
        }
        assert!(!nothrow_callee("js_string_compare_value"));
    }

    #[test]
    fn eh_bookkeeping_gc_polls_and_intrinsics_stay_plain_calls() {
        // The names the deleted hand list carried: the table proves them.
        for name in [
            "js_eh_try_push",
            "js_try_end",
            "js_get_exception",
            "js_clear_exception",
            "js_catch_enter",
            "js_has_exception",
            "js_derived_super_scope_push",
            "js_derived_super_scope_pop",
            "js_gc_safepoint",
            "js_gc_loop_safepoint",
            "js_shadow_slot_bind",
            "js_shadow_frame_enter",
            "llvm.memcpy.p0.p0.i64",
        ] {
            assert!(nothrow_callee(name), "{name}");
        }
        for name in ["js_throw", "js_closure_call0", "js_to_primitive"] {
            assert!(!nothrow_callee(name), "{name}");
        }
    }

    #[test]
    fn phi_predecessors_follow_the_split_tail() {
        let ir = "entry:\n\
                  \x20 invoke void @f() to label %eh.cont1 unwind label %lpad\n\
                  eh.cont1:\n\
                  \x20 invoke void @g() to label %eh.cont2 unwind label %lpad\n\
                  eh.cont2:\n\
                  \x20 br label %merge\n\
                  other:\n\
                  \x20 br label %merge\n\
                  merge:\n\
                  \x20 %r9 = phi double [ %r1, %entry ], [ 1.0, %other ]\n\
                  \x20 ret double %r9\n";
        let out = rewrite_phi_predecessors(ir);
        assert!(out.contains("phi double [ %r1, %eh.cont2 ], [ 1.0, %other ]"));
        // Branch targets keep naming the header.
        assert!(out.contains("br label %merge"));
    }

    #[test]
    fn no_splits_is_identity() {
        let ir = "entry:\n  br label %m\nm:\n  %p = phi double [ 1.0, %entry ]\n  ret double %p\n";
        assert_eq!(rewrite_phi_predecessors(ir), ir);
    }

    #[test]
    fn unsplit_predecessor_pairs_are_untouched() {
        let ir = "a:\n\
                  \x20 invoke void @f() to label %eh.cont7 unwind label %l\n\
                  eh.cont7:\n\
                  \x20 br label %m\n\
                  b:\n\
                  \x20 br label %m\n\
                  m:\n\
                  \x20 %p = phi i64 [ %x, %a ], [ %y, %b ]\n";
        let out = rewrite_phi_predecessors(ir);
        assert!(out.contains("[ %x, %eh.cont7 ], [ %y, %b ]"));
    }
}
