//! Value bridge between NaN-boxed JSValue and V8 values
//!
//! This module handles conversion between the Perry runtime's NaN-boxed
//! representation and V8's value system.
//!
//! ## V8 Object Handle Table
//!
//! V8 objects (objects, arrays, functions) returned to native code are stored
//! in a thread-local handle table. The native code receives a handle ID that
//! can be used to retrieve the V8 object for subsequent operations.

use deno_core::v8;
use perry_runtime::JSValue;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use crate::interop::{bump_js_handle_released, bump_js_handle_stored, bump_v8_entry, V8EntryKind};

// NaN-boxing constants (must match perry-runtime/src/value.rs)
pub(super) const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
pub(super) const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
pub(super) const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
pub(super) const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
pub(super) const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
pub(super) const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
pub(super) const SHORT_STRING_TAG: u64 = 0x7FF9_0000_0000_0000;
pub(super) const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
pub(super) const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;

/// Tag for V8 object handles - these are opaque references to V8 objects
/// stored in the handle table, NOT native Perry objects
pub(super) const JS_HANDLE_TAG: u64 = 0x7FFB_0000_0000_0000;

pub(super) const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
pub(super) const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

// Thread-local storage for V8 object handles
thread_local! {
    /// Maps handle IDs to V8 Global handles
    pub(super) static JS_OBJECT_HANDLES: RefCell<HashMap<u64, v8::Global<v8::Value>>> = RefCell::new(HashMap::new());
    /// Stable V8 constructor-like wrappers for Perry class references.
    pub(super) static NATIVE_CLASS_HANDLES: RefCell<HashMap<u32, v8::Global<v8::Value>>> = RefCell::new(HashMap::new());
    /// Stable V8 function wrappers for Perry closures. Keyed on the raw
    /// `*const ClosureHeader` pointer so the SAME Perry closure crossing into
    /// V8 twice surfaces as the SAME `v8::Function` instance — load-bearing for
    /// `reflect-metadata`'s WeakMap lookups (decorator records metadata on
    /// `descriptor.value`; NestJS RouterExplorer reads it back via
    /// `prototype['methodName']` — both must hash to the same WeakMap key).
    /// (#1021 NestJS decorator-routing blocker.)
    pub(super) static NATIVE_CLOSURE_HANDLES: RefCell<HashMap<usize, v8::Global<v8::Value>>> = RefCell::new(HashMap::new());
    /// V8 Promise resolvers waiting on native Perry promises returned through callbacks.
    pub(super) static NATIVE_PROMISE_RESOLVERS: RefCell<HashMap<u64, v8::Global<v8::PromiseResolver>>> = RefCell::new(HashMap::new());
    /// Snapshot of untampered intrinsics used by the conservative JS export
    /// data-object fast path. Captured during `js_runtime_init`, before user
    /// modules can replace `globalThis.Object` or its methods.
    pub(super) static EXPORT_SNAPSHOT_INTRINSICS: RefCell<Option<ExportSnapshotIntrinsics>> = const { RefCell::new(None) };
    /// Counter for generating unique handle IDs
    pub(super) static NEXT_HANDLE_ID: Cell<u64> = const { Cell::new(1) };
    pub(super) static NEXT_NATIVE_PROMISE_RESOLVER_ID: Cell<u64> = const { Cell::new(1) };
}

pub(super) struct ExportSnapshotIntrinsics {
    pub(super) object_prototype: v8::Global<v8::Value>,
    pub(super) object_is_frozen: v8::Global<v8::Function>,
}

pub fn capture_export_snapshot_intrinsics(scope: &mut v8::PinScope<'_, '_>) {
    let Some(intrinsics) = load_export_snapshot_intrinsics(scope) else {
        // If the lookup of `globalThis.Object` / its `prototype` / `isFrozen`
        // ever fails at runtime init, every export-data-object fast-path
        // eligibility check will silently return false (`is_plain_object`
        // requires the intrinsics cell to be set). That would manifest as a
        // perf cliff rather than a correctness bug — surface it loudly so
        // regressions don't hide as "slow but still working".
        eprintln!(
            "perry-jsruntime: failed to capture Object intrinsics at init; \
             JS export-data-object snapshot fast path disabled \
             (every export read will go through V8 fallback)"
        );
        return;
    };
    EXPORT_SNAPSHOT_INTRINSICS.with(|cell| {
        *cell.borrow_mut() = Some(intrinsics);
    });
}

fn load_export_snapshot_intrinsics(
    scope: &mut v8::PinScope<'_, '_>,
) -> Option<ExportSnapshotIntrinsics> {
    let global = scope.get_current_context().global(scope);
    let object_key = v8::String::new(scope, "Object")?;
    let object_value = global.get(scope, object_key.into())?;
    let object_ctor = v8::Local::<v8::Object>::try_from(object_value).ok()?;

    let prototype_key = v8::String::new(scope, "prototype")?;
    let object_prototype = object_ctor.get(scope, prototype_key.into())?;

    let is_frozen_key = v8::String::new(scope, "isFrozen")?;
    let is_frozen_value = object_ctor.get(scope, is_frozen_key.into())?;
    let object_is_frozen = v8::Local::<v8::Function>::try_from(is_frozen_value).ok()?;

    Some(ExportSnapshotIntrinsics {
        object_prototype: v8::Global::new(scope, object_prototype),
        object_is_frozen: v8::Global::new(scope, object_is_frozen),
    })
}

/// Convert a native string pointer to a Rust String
pub(super) unsafe fn native_string_to_rust(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }

    // StringHeader layout: { utf16_len: u32, byte_len: u32, capacity: u32, refcount: u32, flags: u32, data: [u8] }
    #[repr(C)]
    struct StringHeader {
        _utf16_len: u32,
        byte_len: u32,
        _capacity: u32,
        _refcount: u32,
        _flags: u32,
    }

    let header = ptr as *const StringHeader;
    let length = (*header).byte_len as usize;
    let data_ptr = ptr.add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, length);

    String::from_utf8_lossy(bytes).to_string()
}

/// Convert a Rust string to a native string pointer
pub(super) fn rust_string_to_native(s: &str) -> *const u8 {
    use perry_runtime::js_string_from_bytes;

    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as *const u8
}

mod conversion;
mod fetch_handle;
mod handles;
mod native_class;
mod sqlite_proxy;

pub use conversion::{fixup_native_for_v8, native_to_v8, v8_to_native, v8_to_native_export_value};
pub use handles::{
    get_handle_id, get_js_handle, is_js_handle, make_js_handle_value, release_js_handle,
    store_js_handle,
};
pub use native_class::{v8_to_native_metadata_target, v8_to_native_metadata_value};

// `v8_to_native_array` and `native_array_to_v8` were `pub fn` in the original
// monolithic `bridge.rs` but have no in-tree callers; preserve the
// `crate::bridge::<name>` accessibility through explicit re-exports.
#[allow(unused_imports)]
pub use conversion::{native_array_to_v8, v8_to_native_array};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_constants() {
        // Verify our tag constants match expected values
        assert_eq!(TAG_UNDEFINED, 0x7FFC_0000_0000_0001);
        assert_eq!(TAG_NULL, 0x7FFC_0000_0000_0002);
        assert_eq!(TAG_FALSE, 0x7FFC_0000_0000_0003);
        assert_eq!(TAG_TRUE, 0x7FFC_0000_0000_0004);
    }
}
