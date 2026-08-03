//! TEMPORARY exception-lowering mode switch (#7302).
//!
//! `PERRY_EH=invoke` lowers `try`/`catch` (and the async rejection boundary)
//! to `invoke`/`landingpad` with `js_throw` raising through the Itanium
//! unwinder; unset / `PERRY_EH=setjmp` keeps the setjmp/longjmp lowering.
//!
//! This flag exists ONLY for bisection while the invoke lowering is
//! validated. It is deleted — together with the entire setjmp path
//! (`volatile_setjmp.rs`, `setjmp_abi.rs`, `returns_twice`/`#1` handling) —
//! when the default flips. A permanent hybrid is the failure mode #7302
//! exists to remove; do not build on this switch.
//!
//! The value participates in the object-cache key
//! (`perry/src/commands/compile/object_cache.rs`) — the two modes emit
//! structurally different IR for every function containing a `try`.

use std::collections::HashMap;
use std::sync::OnceLock;

pub(crate) fn invoke_eh_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        matches!(
            std::env::var("PERRY_EH").as_deref(),
            Ok("invoke") | Ok("INVOKE") | Ok("1")
        )
    })
}

/// Runtime helpers that participate in the EH machinery itself and are
/// verified never to reach `js_throw` — they stay plain `call`s inside a
/// protected region (an `invoke` on them would be legal but wires the EH
/// bookkeeping into its own landing pads, which is both noise and, for the
/// catch-entry sequence, a self-referential shape).
///
/// `js_shadow_*` (GC shadow-stack bookkeeping: TLS pushes/pops/stores) and
/// `js_gc_*` (collection entry points) cannot throw JS by construction —
/// the GC has no throw path; allocation failure is a Rust abort.
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

pub(crate) fn callee_is_nothrow(name: &str) -> bool {
    name.starts_with("llvm.")
        || name.starts_with("js_shadow_")
        || name.starts_with("js_gc_")
        || matches!(
            name,
            "js_eh_try_push"
                | "js_try_end"
                | "js_get_exception"
                | "js_clear_exception"
                | "js_has_exception"
        )
        || !crate::module::helper_decl_attrs(name).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

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
