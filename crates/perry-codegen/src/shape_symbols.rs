//! The per-module list of LINK-ASSIGNED shape symbols a codegen run emitted.
//!
//! The driver assigns ShapeIds once per link, over the union of every
//! `perry_shape_abs_*` name the program's modules declare, and emits one
//! generated object that defines them as absolute symbols. To do that it needs
//! the names, and it needs them for modules served from the object cache too —
//! where codegen never ran.
//!
//! Re-deriving the names in the driver from HIR was considered and rejected:
//! the list includes IMPORTED CLASS STUBS, which are named with the IMPORTING
//! module's prefix and are computed from `CompileOptions` inside codegen, not
//! from the module's own HIR. A re-derivation that missed one would leave an
//! undefined symbol at link time. Capturing what codegen actually emitted
//! cannot diverge from what codegen actually emitted.
//!
//! Modelled on `ext_registry`'s `MODULE_CAPTURE`: a thread-local the driver
//! opens around `compile_module` on the same rayon worker that runs it.

use std::cell::RefCell;

thread_local! {
    static CAPTURE: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
}

/// Start capturing on this thread, discarding anything a previous run left.
pub fn begin_module_capture() {
    CAPTURE.with(|c| *c.borrow_mut() = Some(Vec::new()));
}

/// Take what was captured and stop capturing. Empty when capture was never
/// opened, which is the case for every in-process codegen test.
pub fn take_module_capture() -> Vec<String> {
    CAPTURE.with(|c| c.borrow_mut().take()).unwrap_or_default()
}

/// Record one emitted `perry_shape_abs_*` name. A no-op outside a capture.
pub(crate) fn note_static_shape_symbol(name: &str) {
    CAPTURE.with(|c| {
        if let Some(names) = c.borrow_mut().as_mut() {
            names.push(name.to_string());
        }
    });
}
