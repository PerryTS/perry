//! Calls into the optional `perry-wasm-host` archive.
//!
//! The runtime never names a `perry_wasm_host_*` symbol (see
//! `perry_abi::wasm_host`): the host archive registers its entry-point table
//! with [`js_runtime_set_wasm_host_ops`] from the static constructor of the
//! object perry's link step generates for a WebAssembly program, and every
//! wrapper below calls through that table. The wrappers keep the entry
//! points' names and signatures, so the WebAssembly code calls them exactly as
//! it called the archive's symbols.

use std::sync::atomic::{AtomicPtr, Ordering};

#[allow(unused_imports)]
pub(super) use perry_abi::wasm_host::{
    WasmHostOps, WasmImportCallback, WasmImportResolverCallback, WASM_EXTERN_KIND_FUNCTION,
    WASM_EXTERN_KIND_GLOBAL, WASM_EXTERN_KIND_MEMORY, WASM_EXTERN_KIND_TABLE,
    WASM_VAL_KIND_EXTERNREF, WASM_VAL_KIND_F32, WASM_VAL_KIND_F64, WASM_VAL_KIND_I32,
    WASM_VAL_KIND_I64, WASM_VAL_KIND_NONE,
};

static HOST_OPS: AtomicPtr<WasmHostOps> = AtomicPtr::new(std::ptr::null_mut());

/// Register the host archive's entry points. Called by
/// `perry_wasm_host_register` (perry-wasm-host) before `main`; the table must
/// live for the whole process.
#[no_mangle]
pub extern "C" fn js_runtime_set_wasm_host_ops(ops: *const WasmHostOps) {
    HOST_OPS.store(ops as *mut WasmHostOps, Ordering::Release);
}

/// Whether a host archive registered its entry points.
pub(crate) fn host_linked() -> bool {
    !HOST_OPS.load(Ordering::Acquire).is_null()
}

#[inline]
fn ops() -> &'static WasmHostOps {
    let ops = HOST_OPS.load(Ordering::Acquire);
    if ops.is_null() {
        crate::fs::validate::throw_error_with_code(
            "WebAssembly: this executable was linked without the WebAssembly host \
             (perry-wasm-host); compile with --enable-wasm-runtime",
            "ERR_WASM_HOST_NOT_LINKED",
        );
    }
    // SAFETY: only `js_runtime_set_wasm_host_ops` stores, and it stores a
    // table that lives for the process.
    unsafe { &*ops }
}

macro_rules! define_wrappers {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
        $(
            #[allow(dead_code, clippy::too_many_arguments, clippy::unused_unit)]
            pub(crate) unsafe fn $name($($arg: $ty),*) -> $ret {
                (ops().$name)($($arg),*)
            }
        )*
    };
}
perry_abi::wasm_host_entry_points!(define_wrappers);
