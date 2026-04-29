//! Static plugin registry for pre-compiled extensions
//!
//! When Perry compiles with --bundle-extensions, extension modules are compiled
//! into the binary. This registry maps source paths to their default exports
//! so the host can resolve pre-compiled plugins without dynamic loading.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::string::StringHeader;

/// NaN-boxed undefined value
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

thread_local! {
    static STATIC_PLUGINS: RefCell<HashMap<String, f64>> = RefCell::new(HashMap::new());
}

/// Helper: read a StringHeader pointer as a Rust &str
unsafe fn string_as_str<'a>(s: *const StringHeader) -> &'a str {
    let len = (*s).byte_len as usize;
    let data = (s as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8_unchecked(bytes)
}

/// Register a pre-compiled plugin by its canonical source path.
/// Called from the entry module's init function for each bundled extension.
#[no_mangle]
pub extern "C" fn perry_register_static_plugin(path: *const StringHeader, export_value: f64) {
    let path_str = unsafe { string_as_str(path) }.to_string();
    STATIC_PLUGINS.with(|m| m.borrow_mut().insert(path_str, export_value));
}

/// Look up a pre-compiled plugin by its canonical source path.
/// Returns the plugin's default export (NaN-boxed value) or undefined.
#[no_mangle]
pub extern "C" fn perry_resolve_static_plugin(path: *const StringHeader) -> f64 {
    let path_str = unsafe { string_as_str(path) };
    STATIC_PLUGINS.with(|m| {
        m.borrow()
            .get(path_str)
            .copied()
            .unwrap_or(f64::from_bits(TAG_UNDEFINED))
    })
}
