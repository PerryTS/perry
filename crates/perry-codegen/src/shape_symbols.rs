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

/// Is design step 4 on for this compile?
///
/// An environment gate rather than a `CompileOptions` field, for the reason
/// `PERRY_DISABLE_BUFFER_FAST_PATH` and `PERRY_VERIFY_NATIVE_REGIONS` are:
/// `CompileOptions` is constructed exhaustively at ~100 sites across the
/// codegen test suites, and a new field there is a hundred-file edit that
/// would collide with every lane currently in flight for no behavioural gain.
///
/// The object cache still covers it: `compute_object_cache_key_with_env`
/// already folds this family of variables into the key by name, and the driver
/// SETS the variable before codegen whenever the link that must define the
/// absolute symbols is not going to happen (`--no-link`, bitcode-link mode,
/// COFF targets). Read fresh rather than memoised so a test can compile both
/// arms in one process.
pub fn link_time_shape_ids_enabled() -> bool {
    std::env::var("PERRY_NO_LINKTIME_SHAPE_IDS").ok().as_deref() != Some("1")
}

/// Is design step 4 on for THIS target?
///
/// x86-64 only, and the restriction is about relocations, not about effort.
/// Measured with LLVM 22 on each target's own lowering:
///
/// * **x86-64** — `external hidden` + a FULL `!absolute_symbol` range gives
///   `movabsq $sym, %r` (`R_X86_64_64`), which GNU ld 2.42 and lld both accept
///   in a PIE, and which LICM hoists out of a read loop. A NARROW range gives
///   the one-instruction `cmpl $sym, 4(%rdi)` instead, but that needs
///   `R_X86_64_32`, and bfd refuses that against an absolute symbol in a PIE —
///   which is the link perry's native Linux target performs.
/// * **aarch64** — `hidden` makes LLVM materialise the symbol PC-relatively
///   (`adrp` + `add :lo12:`), which is WRONG for an absolute symbol: the
///   linker would have to reach 0x8000_0000 from the text segment. Default
///   visibility with a narrow range gives a GOT load instead (`adrp :got:` +
///   `ldr`), which is correct, is two instructions rather than one, and keeps
///   the sharing and hoisting that are the larger half of this change. That
///   form is not enabled here because it has not been linked and run on an
///   arm64 host — the difference between "the lowering looks right" and "it
///   links and returns the right value" is the whole reason this lane exists.
/// * everything else (wasm, riscv, i686, COFF) — unverified, so off.
///
/// Being off is not a fallback path: the module emits no absolute symbols,
/// module init mints as it does today, and no read site changes.
pub fn link_time_shape_ids_for_target(triple: &str) -> bool {
    link_time_shape_ids_enabled() && triple.starts_with("x86_64")
}
