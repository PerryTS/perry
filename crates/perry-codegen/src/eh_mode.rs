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
