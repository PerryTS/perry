//! Armed dispatch for native-module NAMESPACE object behaviors (binary size).
//!
//! Generic object paths (`Object.getOwnPropertyDescriptor`, dynamic field
//! stores, `Reflect.defineProperty` redefine checks, own-key enumeration)
//! special-case objects with `class_id == NATIVE_MODULE_CLASS_ID`. Referencing
//! the native-module implementations statically from those always-linked
//! paths pinned the entire module-namespace web — `callable_exports`,
//! `constants`, the submodule export tables, and through them the
//! stream/sqlite/buffer/assert surfaces — into every binary, including ones
//! that import no module at all (measured on a `console.log` hello world via
//! `ld64 -why_live`).
//!
//! Instead, the generic paths call through this armed function-pointer table.
//! `install_native_module_vtable()` — reached from every path that can mint a
//! namespace-classed object, and nothing else — arms it, so:
//! - a program with no native-module import never links the implementations
//!   (the ops static lives on the native-module side and is dead with it);
//! - a program that CAN observe a namespace object arms the table before the
//!   first such object exists, so behavior is identical to the old static
//!   calls (unarmed + matching class_id is unreachable: the class id is only
//!   ever assigned by the same bootstrap that arms).
use std::sync::atomic::{AtomicPtr, Ordering};

use super::ObjectHeader;
use crate::string::StringHeader;

pub(crate) struct NmNamespaceOps {
    /// `Object.getOwnPropertyDescriptor` on a namespace object. Returns the
    /// NaN-boxed descriptor object, or `None` to fall through to the generic
    /// own-property handling.
    pub get_own_descriptor:
        unsafe fn(*mut ObjectHeader, *const StringHeader, key_name: Option<&str>) -> Option<f64>,
    /// Dynamic field store on a namespace object (CommonJS namespaces are
    /// mutable in Node — overrides are recorded, not thrown). Returns true
    /// when the store was fully handled.
    pub field_set_override: unsafe fn(*mut ObjectHeader, *const StringHeader, f64) -> bool,
    /// `Reflect.defineProperty`-style "is this an existing own key" probe.
    /// The key is the NaN-boxed property key value.
    pub reflect_has_enumerable: unsafe fn(*mut ObjectHeader, f64) -> bool,
    /// Own-key enumeration (`Object.keys` / `getOwnPropertyNames` clone
    /// paths). `None` when the value is not a namespace object.
    pub own_keys_array: unsafe fn(*const ObjectHeader) -> Option<*mut crate::array::ArrayHeader>,
    /// Bound-method materialization for a dynamic property get on a
    /// namespace object (`const fn = fs.lstatSync`): NaN-boxed namespace
    /// value + property-name bytes → bound callable (or undefined).
    pub bind_method: unsafe fn(f64, *const u8, usize) -> f64,
}

static NM_NAMESPACE_OPS: AtomicPtr<NmNamespaceOps> = AtomicPtr::new(std::ptr::null_mut());

pub(crate) fn arm_nm_namespace_ops(ops: &'static NmNamespaceOps) {
    // `black_box` for the same reason as `NM_INSTALL_ALL_HOOK` / the fs
    // thread codec: whole-program opt speculatively devirtualizes a
    // single-store AtomicPtr back into direct references, re-pinning
    // everything this table exists to unpin.
    NM_NAMESPACE_OPS.store(
        std::hint::black_box(ops as *const NmNamespaceOps as *mut NmNamespaceOps),
        Ordering::Release,
    );
}

#[inline]
pub(crate) fn nm_namespace_ops() -> Option<&'static NmNamespaceOps> {
    let p = NM_NAMESPACE_OPS.load(Ordering::Acquire);
    if p.is_null() {
        None
    } else {
        // SAFETY: only ever stores `&'static NmNamespaceOps`.
        Some(unsafe { &*p })
    }
}
