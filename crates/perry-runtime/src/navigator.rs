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

use crate::array::js_array_push_f64;
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

fn normalize_navigator_locale(locale: &str) -> Option<String> {
    let (locale, modifier) = locale.split_once('@').unwrap_or((locale, ""));
    let locale = locale.split('.').next()?;
    if locale.is_empty() || matches!(locale, "C" | "POSIX") {
        return None;
    }

    let mut tag = locale.replace('_', "-");
    if !modifier.is_empty() {
        tag.push('-');
        // POSIX modifiers with an invalid BCP-47 variant shape are exposed by
        // Node through the private `x-lvariant-` sequence (`@euro`).
        let is_variant = ((5..=8).contains(&modifier.len())
            || (modifier.len() == 4 && modifier.as_bytes()[0].is_ascii_digit()))
            && modifier.bytes().all(|byte| byte.is_ascii_alphanumeric());
        if !is_variant {
            tag.push_str("x-lvariant-");
        }
        tag.push_str(modifier);
    }
    crate::intl::canonical_locale(&tag)
}

fn navigator_language() -> String {
    std::env::var("LC_ALL")
        .ok()
        .filter(|locale| !locale.is_empty() && locale != "C" && locale != "POSIX")
        .or_else(|| std::env::var("LANG").ok())
        .as_deref()
        .and_then(normalize_navigator_locale)
        .unwrap_or_else(|| "en-US".to_string())
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(constructor);
    let prototype_key = scope.root_nanbox_f64(f64::from_bits(
        crate::value::js_nanbox_string(js_string_from_bytes(b"prototype".as_ptr(), 9) as i64)
            .to_bits(),
    ));
    let prototype = f64::from_bits(
        crate::object::js_object_get_field_by_name(
            JSValue::from_bits(constructor.get_nanbox_u64())
                .as_pointer::<crate::closure::ClosureHeader>()
                as *const crate::object::ObjectHeader,
            JSValue::from_bits(prototype_key.get_nanbox_u64()).as_string_ptr(),
        )
        .bits(),
    );
    let prototype = scope.root_nanbox_f64(prototype);

    // Packed null-separated keys; slot order must match the set_field calls.
    let packed = b"userAgent\0language\0languages\0hardwareConcurrency\0platform\0locks\0";
    let field_count: u32 = 6;
    let obj = crate::realm_decl! { birth js_object_alloc_with_shape(
        NAVIGATOR_CLASS_ID,
        field_count,
        packed.as_ptr(),
        packed.len() as u32,
    ); realm { navigator, kind = namespace, extends = Navigator.prototype, members = { userAgent = "Node.js/22", attrs = writable | enumerable | configurable; language = runtime(navigator.language), attrs = writable | enumerable | configurable; languages, attrs = writable | enumerable | configurable; hardwareConcurrency = runtime(navigator.hardwareConcurrency), attrs = writable | enumerable | configurable; platform = target(navigator.platform), attrs = writable | enumerable | configurable; locks, attrs = writable | enumerable | configurable; }; } };
    let obj = scope.root_raw_mut_ptr(obj);
    obj.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| unsafe {
        (*obj).class_id = NAVIGATOR_CLASS_ID;
        // Charter step 3: `class_id` is a store-kind input.
        crate::object::shapes::store_kind::restamp_object_store_kind(obj);
    });
    // The passed constructor is authoritative. Avoid fetching `Navigator` from
    // globalThis here: globalThis construction itself calls this helper.
    crate::object::js_object_set_prototype_of(
        js_nanbox_pointer(obj.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| obj as i64)),
        prototype.get_nanbox_f64(),
    );

    // userAgent: "Node.js/<major>"
    let ua = format!("Node.js/{NODE_MAJOR}");
    obj.with_mut_ptr(|obj| js_object_set_field(obj, 0, nb_str(&ua)));

    // language / languages
    let language = navigator_language();
    obj.with_mut_ptr(|obj| js_object_set_field(obj, 1, nb_str(&language)));
    let langs = scope.root_raw_mut_ptr(crate::realm_decl! { birth crate::array::realm_array_alloc(1, perry_abi::realm_header::ArrayElements::Pointers, false); realm { navigator.languages, value = dynamic, elements = string } });
    let lang = nb_str(&language);
    let pushed = langs.with_mut_ptr(|arr| js_array_push_f64(arr, f64::from_bits(lang.bits())));
    langs.set_raw_mut_ptr(pushed);
    obj.with_mut_ptr(|obj| {
        js_object_set_field(
            obj,
            2,
            JSValue::from_bits(
                js_nanbox_pointer(
                    langs.with_mut_ptr::<crate::array::ArrayHeader, _>(|arr| arr as i64),
                )
                .to_bits(),
            ),
        )
    });

    // hardwareConcurrency: recommended parallelism (>= 1).
    let cores = std::thread::available_parallelism()
        .map(|n| n.get() as f64)
        .unwrap_or(1.0);
    obj.with_mut_ptr(|obj| js_object_set_field(obj, 3, JSValue::number(cores)));

    // platform: browser-style token.
    obj.with_mut_ptr(|obj| js_object_set_field(obj, 4, nb_str(navigator_platform())));

    // locks: a plain object stub (typeof === "object"). Web Locks API is not
    // implemented; the property exists so feature-detection sees the shape.
    let locks = crate::realm_decl! { birth crate::object::js_object_alloc(0, 0); realm { navigator.locks, kind = namespace, members = { }; } };
    obj.with_mut_ptr(|obj| {
        js_object_set_field(
            obj,
            5,
            JSValue::from_bits(js_nanbox_pointer(locks as i64).to_bits()),
        )
    });

    js_nanbox_pointer(obj.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| obj as i64))
}

#[cfg(test)]
pub(crate) fn test_navigator_object_with_constructor(constructor: f64) -> f64 {
    navigator_object_with_constructor(constructor)
}

#[cfg(test)]
mod tests {
    use super::normalize_navigator_locale;

    #[test]
    fn normalizes_posix_locale_modifiers() {
        assert_eq!(
            normalize_navigator_locale("de_DE@euro"),
            Some("de-DE-x-lvariant-euro".into())
        );
        assert_eq!(
            normalize_navigator_locale("de_DE.UTF-8@euro"),
            Some("de-DE-x-lvariant-euro".into())
        );
        assert_eq!(
            normalize_navigator_locale("zh_CN@pinyin"),
            Some("zh-CN-pinyin".into())
        );
        assert_eq!(
            normalize_navigator_locale("bogus-locale"),
            Some("bogus-locale".into())
        );
        assert_eq!(normalize_navigator_locale("C.UTF-8"), None);
    }
}
