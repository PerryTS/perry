//! The unresolved-module namespace stub — the "empty object" handed to user
//! code when a module import or a method dispatch has nowhere to go.
//!
//! Split out of `object/mod.rs` (2000-line cap) by #8113.
//!
//! # Honest tags (#340/#341, #10821 row 4)
//!
//! This used to be a `static NullObjectBytes` — a `.data` byte array laid out
//! like an `ObjectHeader`, whose ADDRESS was handed to JS under `POINTER_TAG`.
//! It looked like an object to everything that reads an `ObjectHeader`, and it
//! is not one: **it has no `GcHeader`**. `addr_class::try_read_gc_header`
//! accepts any heap-plausible address and returns `&*((addr - 8) as *const
//! GcHeader)`, so every brand probe on this value read whatever `.data` bytes
//! happened to precede the static and dispatched on them as an `obj_type`.
//! That is the same hazard `native_call_method.rs` already documents for a
//! `Box`-allocated `SymbolHeader`, and it is what the honest-tag invariant —
//! a `POINTER_TAG` value is always a dereferenceable GC cell — exists to
//! forbid. It worked only because the preceding bytes happened to be benign.
//!
//! The stub is now an ordinary `GC_TYPE_OBJECT` with class id 0 and zero own
//! keys: exactly what `{}` allocates, so `typeof`, `Object.keys`,
//! `JSON.stringify` and property reads are unchanged, and the header at
//! `addr - 8` is real.
//!
//! It stays ONE object per realm, because that is what it was: every stub was
//! the same static address, so every stub was `===` every other. Per realm
//! rather than per process because a GC object belongs to the thread whose
//! arena allocated it — a static was shared across threads, which a heap
//! object must not be.

use std::sync::atomic::{AtomicI64, Ordering};

crate::perry_thread_local! {
    static NULL_STUB_SLOT: AtomicI64 = const { AtomicI64::new(0) };
}

/// The realm's stub object. A GC pointer in a static, so it is scanned from
/// `object::scan_object_cache_roots_mut` beside the iterator tower — both to
/// keep it alive and to rewrite the slot when a moving collection relocates
/// it. Without the rewrite every later stub would be a stale address, which is
/// strictly worse than the static it replaces.
pub(crate) static NULL_STUB_PTR: crate::object::RealmAtomicI64 =
    crate::object::RealmAtomicI64::new(&NULL_STUB_SLOT);

/// GC root for the stub singleton.
pub(crate) fn scan_null_stub_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    NULL_STUB_PTR.with_slot(|slot| {
        visitor.visit_atomic_i64_slot(slot, Ordering::Acquire, Ordering::Release);
    });
}

/// The realm's unresolved-namespace stub, allocating it on first use.
///
/// Lazy, so a program that never hits an unresolved import pays nothing — and
/// every one of the eleven call sites returns this value immediately, with no
/// raw receiver pointer live across it, which is what makes allocating from
/// inside the property-read funnels safe here.
pub(crate) fn null_stub_object() -> *mut super::ObjectHeader {
    let existing = NULL_STUB_PTR.load(Ordering::Acquire);
    if existing != 0 {
        return existing as *mut super::ObjectHeader;
    }
    // Class id 0 and zero keys: an ordinary `{}`. Deliberately NOT a family
    // class id — the stub carries no native state, so it must stay an
    // ordinary object that a worker can deep-copy like any other `{}`.
    let obj = super::js_object_alloc(0, 0);
    if obj.is_null() {
        return std::ptr::null_mut();
    }
    NULL_STUB_PTR.store(obj as i64, Ordering::Release);
    obj
}

/// The stub as a JS value. The single funnel every fallback returns through.
///
/// Answers `undefined` if the allocation fails, which is the honest behaviour
/// under memory exhaustion: a caller then sees "cannot read property of
/// undefined" rather than dereferencing a null pointer, and the static this
/// replaces could not report failure at all.
pub(crate) fn null_stub_value() -> f64 {
    let obj = null_stub_object();
    if obj.is_null() {
        return f64::from_bits(0x7FFC_0000_0000_0001); // TAG_UNDEFINED
    }
    f64::from_bits(crate::JSValue::pointer(obj as *mut u8).bits())
}

/// Issue #629: namespace imports for unresolved modules
/// (`import * as fsp from "node:fs/promises"` when the module isn't
/// implemented) used to fall back to `TAG_TRUE` at the codegen
/// catch-all, which made `typeof fsp === "boolean"` and every
/// `fsp.method` access return undefined silently — confusing because
/// the user sees `(boolean).method is not a function`. Returning a
/// stable empty-object stub makes `typeof === "object"` (matches
/// Node's module-namespace shape) and property access cleanly returns
/// undefined via the existing object-field path.
#[no_mangle]
pub extern "C" fn js_unresolved_namespace_stub() -> f64 {
    if crate::hot_diag::receiver_repr_on() {
        crate::hot_diag::receiver_repr_note_constructed(
            crate::hot_diag::ReceiverReprFamily::NullStub,
        );
    }
    null_stub_value()
}

/// Issue #692: default-import calls against unresolved modules
/// (`import jwt from "jsonwebtoken"; jwt.sign(...)` when no perry-stdlib
/// binding matched the method, or `import sanitizeHtml from
/// "sanitize-html"; sanitizeHtml(x)` when sanitize-html doesn't resolve
/// to a NativeCompiled module) used to lower to an LLVM extern named
/// literally `default`, which the system linker can't resolve —
/// surfaced as `undefined reference to 'default'`. Route those calls
/// here so the binary links; the runtime stub prints a one-shot
/// diagnostic and returns NaN-boxed undefined. The user gets a clear
/// signal at first call rather than a cryptic link error.
#[no_mangle]
pub extern "C" fn js_unresolved_default_call() -> f64 {
    use std::sync::atomic::{AtomicBool, Ordering};
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        eprintln!(
            "perry: called a default-imported binding from an unresolved module \
             (returns undefined). The module's default export was not found in \
             perry-stdlib or perry.compilePackages — run `perry --print-api-manifest` \
             to see what's supported."
        );
    }
    f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
}

// #340/#341 row 4 deleted `is_null_stub_address`. Its only production caller
// was the receiver-repr ledger's `observe_pointer` arm, which asked whether a
// decoded pointer was the `.data` static — gate A inverts that arm away, and a
// heap object needs no address-equality probe to be recognised.

#[cfg(test)]
mod tests {
    use super::*;

    /// GATE B for this family, and the invariant it exists for: the value JS
    /// receives is a real heap object with a `GcHeader`, not a `.data` static
    /// whose `addr - 8` is whatever the linker put there.
    #[test]
    fn the_stub_is_an_ordinary_object_with_a_real_header() {
        let value = js_unresolved_namespace_stub();
        let bits = value.to_bits();
        assert_eq!(bits & crate::value::TAG_MASK, crate::value::POINTER_TAG);
        let addr = (bits & crate::value::POINTER_MASK) as usize;
        assert!(
            !crate::value::addr_class::is_handle_band(addr),
            "gate B: the stub is in the small-handle band ({addr:#x})"
        );
        let header = unsafe { crate::value::addr_class::try_read_gc_header(addr) }
            .expect("the stub carries a GcHeader");
        assert_eq!(header.obj_type, crate::gc::GC_TYPE_OBJECT);
        let obj = addr as *mut super::super::ObjectHeader;
        assert_eq!(
            unsafe { (*obj).class_id },
            0,
            "an ordinary object, not a family"
        );
        let keys = unsafe { crate::object::object_keys_array(obj) };
        let key_count = if keys.is_null() {
            0
        } else {
            unsafe { (*keys).length }
        };
        assert_eq!(key_count, 0, "the stub must have no own keys");
        assert_eq!(
            NULL_STUB_PTR.load(Ordering::Acquire) as usize,
            addr,
            "the realm slot holds it"
        );
    }

    /// One object per realm, as the static was: every stub was the same
    /// address, so every stub was `===` every other, and a program that
    /// compares two unresolved namespaces must keep seeing that.
    #[test]
    fn every_stub_in_a_realm_is_the_same_object() {
        let a = js_unresolved_namespace_stub();
        let b = js_unresolved_namespace_stub();
        assert_eq!(a.to_bits(), b.to_bits());
    }
}
