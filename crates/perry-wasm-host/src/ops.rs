//! The entry-point table this archive registers with the runtime
//! (`perry_abi::wasm_host`). The runtime archive never names a
//! `perry_wasm_host_*` symbol; it calls this crate only through the table.

use perry_abi::wasm_host::WasmHostOps;

#[allow(unused_imports)]
use super::externals::*;
#[allow(unused_imports)]
use super::*;

extern "C" {
    fn js_runtime_set_wasm_host_ops(ops: *const WasmHostOps);
}

macro_rules! host_ops {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
        WasmHostOps {
            // Each entry point is this crate's `#[no_mangle]` function of the
            // same name. Their handle parameters are typed pointers here and
            // `*mut c_void` in the table; the two are the same C ABI.
            $($name: unsafe {
                std::mem::transmute::<*const (), unsafe extern "C" fn($($ty),*) -> $ret>(
                    $name as *const (),
                )
            },)*
        }
    };
}

/// Register this archive's entry points with the runtime. perry's link step
/// calls it from the static constructor of the object it generates for a
/// program that uses WebAssembly; it only stores a pointer.
#[no_mangle]
pub extern "C" fn perry_wasm_host_register() {
    static OPS: std::sync::OnceLock<WasmHostOps> = std::sync::OnceLock::new();
    let ops = OPS.get_or_init(|| perry_abi::wasm_host_entry_points!(host_ops));
    unsafe { js_runtime_set_wasm_host_ops(ops) };
}
