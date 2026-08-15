//! The unresolved-module namespace stub — a static, GcHeader-less "empty
//! object" handed to user code when a module import or a method dispatch has
//! nowhere to go.
//!
//! Split out of `object/mod.rs` (2000-line cap) by #8113, which also gave the
//! mirror its missing `meta` word.

/// Static "null object" used as a safe return value when the depth guard triggers.
/// Instead of returning undefined (which callers may dereference as a null pointer),
/// we return a pointer to this valid-but-empty object so downstream code doesn't crash.
///
/// Uses a raw byte array with matching layout to avoid Sync issues with raw pointers.
///
/// #8113: mirrors the post-shrink `ObjectHeader` word for word, including the
/// trailing `meta` slot the pre-#8113 spelling omitted (a `(*obj).meta` read on
/// the stub used to run off the end of the static).
#[repr(C, align(8))]
pub(crate) struct NullObjectBytes {
    class_id: u32,        // 0
    parent_class_id: u32, // 0 (never a ShapeId: the stub has no descriptor)
    keys_array: u64,      // 0 (null pointer as u64)
    meta: u64,            // 0 (null pointer as u64)
}
// Safety: this is a read-only zero-initialized struct with no interior mutability
unsafe impl Sync for NullObjectBytes {}

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
    let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
    f64::from_bits(crate::JSValue::pointer(null_obj_ptr).bits())
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

pub(crate) static NULL_OBJECT_BYTES: NullObjectBytes = NullObjectBytes {
    class_id: 0,
    parent_class_id: 0,
    keys_array: 0,
    meta: 0,
};
