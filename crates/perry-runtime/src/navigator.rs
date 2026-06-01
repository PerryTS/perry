//! Node `globalThis.navigator` object (#2923).
//!
//! Node exposes a browser-compatible `navigator` singleton carrying runtime
//! metadata: `userAgent` (`"Node.js/<major>"`), `language` / `languages`,
//! `hardwareConcurrency`, `platform`, and a `locks` stub. Perry installs this
//! object on the `globalThis` singleton (see `object/global_this.rs`) and via
//! the bare-`navigator` global name path so feature-detecting libraries read a
//! Node-shaped object instead of `undefined`.
//!
//! Host-dependent values (`hardwareConcurrency`, `platform`) reflect the real
//! machine; the rest match Node's defaults. The major version mirrors the Node
//! target Perry reports through `process.version` (currently 22).

use crate::array::{js_array_alloc, js_array_push_f64};
use crate::object::{js_object_alloc_with_shape, js_object_set_field};
use crate::string::js_string_from_bytes;
use crate::value::{js_nanbox_pointer, js_nanbox_string, JSValue};

pub const NAVIGATOR_CLASS_ID: u32 = 0x7FFF_FF22;

/// Major Node version Perry advertises (mirrors `process.version` = v22.x).
const NODE_MAJOR: &str = "22";

fn nb_str(s: &str) -> JSValue {
    let bytes = s.as_bytes();
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    JSValue::from_bits(js_nanbox_string(ptr as i64).to_bits())
}

/// Node `navigator.platform` value — the browser-style platform token, which
/// differs from `os.platform()` (`"darwin"` etc).
fn navigator_platform() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "MacIntel"
    }
    #[cfg(target_os = "windows")]
    {
        "Win32"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "Linux x86_64"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "Linux aarch64"
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "windows",
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
    )))]
    {
        "Linux"
    }
}

/// Build the `navigator` object. Fields (positional, matching the packed
/// key order): `userAgent`, `language`, `languages`, `hardwareConcurrency`,
/// `platform`, `locks`.
#[no_mangle]
pub extern "C" fn js_navigator_object() -> f64 {
    let ctor = crate::object::js_get_global_this_builtin_value(b"Navigator".as_ptr(), 9);
    navigator_object_with_constructor(ctor)
}

pub(crate) fn navigator_object_with_constructor(constructor: f64) -> f64 {
    // Packed null-separated keys; slot order must match the set_field calls.
    let packed = b"userAgent\0language\0languages\0hardwareConcurrency\0platform\0locks\0";
    let field_count: u32 = 6;
    let obj = js_object_alloc_with_shape(
        NAVIGATOR_CLASS_ID,
        field_count,
        packed.as_ptr(),
        packed.len() as u32,
    );
    unsafe {
        (*obj).class_id = NAVIGATOR_CLASS_ID;
    }

    // userAgent: "Node.js/<major>"
    let ua = format!("Node.js/{NODE_MAJOR}");
    js_object_set_field(obj, 0, nb_str(&ua));

    // language / languages
    js_object_set_field(obj, 1, nb_str("en-US"));
    let mut langs = js_array_alloc(1);
    langs = js_array_push_f64(langs, f64::from_bits(nb_str("en-US").bits()));
    js_object_set_field(
        obj,
        2,
        JSValue::from_bits(js_nanbox_pointer(langs as i64).to_bits()),
    );

    // hardwareConcurrency: recommended parallelism (>= 1).
    let cores = std::thread::available_parallelism()
        .map(|n| n.get() as f64)
        .unwrap_or(1.0);
    js_object_set_field(obj, 3, JSValue::number(cores));

    // platform: browser-style token.
    js_object_set_field(obj, 4, nb_str(navigator_platform()));

    // locks: a plain object stub (typeof === "object"). Web Locks API is not
    // implemented; the property exists so feature-detection sees the shape.
    let locks = crate::object::js_object_alloc(0, 0);
    js_object_set_field(
        obj,
        5,
        JSValue::from_bits(js_nanbox_pointer(locks as i64).to_bits()),
    );

    // constructor: the singleton should identify as an instance of the
    // global `Navigator` constructor.
    let ctor_key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    crate::object::js_object_set_field_by_name(obj, ctor_key, constructor);

    js_nanbox_pointer(obj as i64)
}

#[cfg(test)]
pub(crate) fn test_navigator_object_with_constructor(constructor: f64) -> f64 {
    navigator_object_with_constructor(constructor)
}
