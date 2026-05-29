//! Deferred refusals for tree-shaking (#2309).
//!
//! Perry compiles every import-reachable module and refuses (hard-errors)
//! during lowering when a module contains a genuinely-runtime `new Function`
//! (the [`crate::eval_classifier`] `RuntimeUnknown` bucket) or an
//! unimplemented Node/Web API (#463). When tree-shaking is enabled, a module
//! that is reachable only via a dead barrel edge or a dead `process.env`
//! branch will be *pruned* before codegen — so refusing on it during
//! collection is premature: the offending code never ships.
//!
//! This module provides a thread-local **deferral sink**. The compile driver
//! arms it around the lowering of a `node_modules` module (and only when the
//! tree-shake flag is on); while armed, refusal sites record a
//! [`DeferredRefusal`] and fall through to their legacy lowering instead of
//! erroring. After the full module graph is collected and reachability is
//! computed, the driver re-raises any deferred refusal whose module survived
//! the prune (see `compile::reachability`). A refusal in a *surviving* module
//! is still a hard error — we never silently ship broken code.
//!
//! User/host source is never armed, so refusals the user wrote stay fatal and
//! span-precise exactly as before. With the flag off the sink is never armed,
//! so behaviour is byte-identical to today.

use std::cell::RefCell;

/// A refusal recorded during lowering of a `node_modules` module while the
/// deferral sink was armed. The module path is filled in by the compile
/// driver after lowering (it knows the canonical path of the module it just
/// lowered); lowering only knows the message + line.
#[derive(Debug, Clone)]
pub struct DeferredRefusal {
    /// Canonical path of the module the refusal came from. Empty when first
    /// recorded by lowering; the driver tags it after the lower returns.
    pub module: String,
    /// 1-based source line of the offending site, if resolvable.
    pub line: Option<usize>,
    /// The original refusal message (re-raised verbatim if the module
    /// survives pruning).
    pub message: String,
}

thread_local! {
    /// `Some(vec)` when armed. Lowering pushes refusals here instead of
    /// erroring. `None` (default) means refusals are fatal as usual.
    static DEFERRAL_SINK: RefCell<Option<Vec<DeferredRefusal>>> = const { RefCell::new(None) };
}

/// Arm the deferral sink on the current thread. Call before lowering a
/// `node_modules` module when tree-shaking is enabled; pair with
/// [`disarm_deferral_sink`]. Re-arming clears any prior contents.
pub fn arm_deferral_sink() {
    DEFERRAL_SINK.with(|s| *s.borrow_mut() = Some(Vec::new()));
}

/// Disarm the sink and return everything recorded while it was armed. Returns
/// an empty vec if it was not armed.
pub fn disarm_deferral_sink() -> Vec<DeferredRefusal> {
    DEFERRAL_SINK.with(|s| s.borrow_mut().take().unwrap_or_default())
}

/// Try to defer a refusal. Returns `true` if the sink was armed (the refusal
/// was recorded and the caller should fall through to its legacy lowering),
/// `false` otherwise (the caller should raise its normal hard error).
///
/// `byte_offset` is `span.lo.0`, used to resolve a source line via the
/// active `CURRENT_MODULE_SOURCE` thread-local.
pub fn try_defer_refusal(message: String, byte_offset: u32) -> bool {
    DEFERRAL_SINK.with(|s| {
        let mut sink = s.borrow_mut();
        if let Some(v) = sink.as_mut() {
            let line = crate::ir::current_module_line_at(byte_offset);
            v.push(DeferredRefusal {
                module: String::new(),
                line,
                message,
            });
            true
        } else {
            false
        }
    })
}
