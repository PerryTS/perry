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
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    {
        return crate::snap_census::input_call("locale.navigator", &format!("{:?}", ()), || {
            crate::effect!(host_read, locale);
            std::env::var("LC_ALL")
                .ok()
                .filter(|locale| !locale.is_empty() && locale != "C" && locale != "POSIX")
                .or_else(|| std::env::var("LANG").ok())
                .as_deref()
                .and_then(normalize_navigator_locale)
                .unwrap_or_else(|| "en-US".to_string())
        });
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    {
        crate::effect!(host_read, locale);
        std::env::var("LC_ALL")
            .ok()
            .filter(|locale| !locale.is_empty() && locale != "C" && locale != "POSIX")
            .or_else(|| std::env::var("LANG").ok())
            .as_deref()
            .and_then(normalize_navigator_locale)
            .unwrap_or_else(|| "en-US".to_string())
    }
}

/// Build the `navigator` object: Node's shape, no own properties. Every
/// member is a `Navigator.prototype` getter ([`install_prototype`]); the host
/// values are computed by the getter on first access and kept in the
/// instance's private fields, as Node's `#languages` / `#availableParallelism`
/// / `#locks` (`lib/internal/navigator.js`). Nothing is read from the host here.
#[no_mangle]
pub extern "C" fn js_navigator_object() -> f64 {
    let ctor = crate::object::js_get_global_this_builtin_value(b"Navigator".as_ptr(), 9);
    navigator_object_with_constructor(ctor)
}

/// `globalThis.navigator` getter (Node: a lazy accessor). The first read builds
/// the object; capture 0 keeps it, so every later read returns the same object.
pub(crate) extern "C" fn global_this_navigator_getter_thunk(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let cached = crate::closure::js_closure_get_capture_bits(closure, 0);
    if cached != crate::value::TAG_UNDEFINED && cached != 0 {
        return f64::from_bits(cached);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure = scope.root_raw_mut_ptr(closure as *mut crate::closure::ClosureHeader);
    let navigator = js_navigator_object();
    closure.with_mut_ptr::<crate::closure::ClosureHeader, _>(|closure| {
        crate::closure::js_closure_set_capture_bits(closure, 0, navigator.to_bits())
    });
    navigator
}

/// The instance's private fields (Node's `#languages`, `#availableParallelism`,
/// `#locks`): born `undefined`, filled by the first read of their getter.
const LANGUAGES: &str = "[[NavigatorLanguages]]";
const PARALLELISM: &str = "[[NavigatorAvailableParallelism]]";
const LOCKS: &str = "[[NavigatorLocks]]";

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

    // The R node `navigator` is the global key as well as this object, and
    // the key's value is the accessor `navigator.<get>` (born with the key
    // when the realm is populated), so reaching the key births the getter.
    let obj = crate::realm_decl! { birth crate::object::js_object_alloc(0, 3); realm { navigator, kind = namespace, extends = Navigator.prototype, members = { }, births = ["navigator.<get>"]; } };
    let obj = scope.root_raw_mut_ptr(obj);
    obj.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| unsafe {
        (*obj).class_id = NAVIGATOR_CLASS_ID;
        // Charter step 3: `class_id` is a store-kind input.
        crate::object::shapes::store_kind::restamp_object_store_kind(obj);
    });
    let boxed =
        || js_nanbox_pointer(obj.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| obj as i64));
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    for field in [LANGUAGES, PARALLELISM, LOCKS] {
        crate::object::intrinsic_private_add(boxed(), field, undefined);
    }
    // The passed constructor is authoritative. Avoid fetching `Navigator` from
    // globalThis here: globalThis construction itself calls this helper.
    crate::object::js_object_set_prototype_of(boxed(), prototype.get_nanbox_f64());
    boxed()
}

/// The receiver's private field `field`; a receiver without it is not a
/// navigator: Node's brand check (`#<node_field>`) throws a TypeError.
fn private_field(this: crate::closure::JsThis, field: &'static str, node_field: &str) -> f64 {
    match crate::object::intrinsic_private_get(this.as_f64(), field) {
        Some(value) => value,
        None => {
            let message = format!(
                "Cannot read private member #{node_field} from an object whose class did not declare it"
            );
            let message = js_string_from_bytes(message.as_ptr(), message.len() as u32);
            crate::exception::throw_core(crate::error::CoreError::type_error(message))
        }
    }
}

fn is_undefined(value: f64) -> bool {
    value.to_bits() == crate::value::TAG_UNDEFINED
}

/// `#languages ??= Object.freeze([locale])`: one frozen array per instance.
fn languages(this: crate::closure::JsThis, node_field: &str) -> f64 {
    let cached = private_field(this, LANGUAGES, node_field);
    if !is_undefined(cached) {
        return cached;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(this.as_f64());
    let language = navigator_language();
    let langs = scope.root_raw_mut_ptr(crate::realm_decl! { birth crate::array::realm_array_alloc(1, perry_abi::realm_header::ArrayElements::Pointers, false); realm { navigator.languages, value = dynamic, elements = string } });
    let lang = nb_str(&language);
    let pushed = langs.with_mut_ptr(|arr| js_array_push_f64(arr, f64::from_bits(lang.bits())));
    langs.set_raw_mut_ptr(pushed);
    let array =
        langs.with_mut_ptr::<crate::array::ArrayHeader, _>(|arr| js_nanbox_pointer(arr as i64));
    let array = scope.root_nanbox_f64(crate::object::js_object_freeze(array));
    crate::object::intrinsic_private_set(
        receiver.get_nanbox_f64(),
        LANGUAGES,
        array.get_nanbox_f64(),
    );
    array.get_nanbox_f64()
}

extern "C" fn get_language(
    _: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let array = JSValue::from_bits(languages(this, "languages").to_bits());
    crate::array::js_array_get_f64(array.as_pointer::<crate::array::ArrayHeader>(), 0)
}

extern "C" fn get_languages(
    _: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    languages(this, "languages")
}

/// `#availableParallelism ??= getAvailableParallelism()` (>= 1).
extern "C" fn get_hardware_concurrency(
    _: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let cached = private_field(this, PARALLELISM, "availableParallelism");
    if !is_undefined(cached) {
        return cached;
    }
    let cores = available_parallelism();
    crate::object::intrinsic_private_set(this.as_f64(), PARALLELISM, cores);
    cores
}

/// Node `getAvailableParallelism()`: the recommended parallelism (>= 1).
fn available_parallelism() -> f64 {
    crate::effect!(host_read, parallelism);
    // The affinity mask and cpu cgroup reads are the Parallelism funnel; the
    // value is a recorded host input (SNAP C2/C3).
    crate::snap_funnel!(Parallelism);
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    {
        crate::snap_census::input_call("navigator.hardwareConcurrency", "()", || {
            std::thread::available_parallelism()
                .map(|n| n.get() as f64)
                .unwrap_or(1.0)
        })
    }
    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    {
        std::thread::available_parallelism()
            .map(|n| n.get() as f64)
            .unwrap_or(1.0)
    }
}

/// `userAgent`: `"Node.js/<major>"`, a constant.
extern "C" fn get_user_agent(
    _: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    private_field(this, LANGUAGES, "userAgent");
    f64::from_bits(nb_str(&format!("Node.js/{NODE_MAJOR}")).bits())
}

/// `platform`: the browser-style token of the build target.
extern "C" fn get_platform(
    _: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    private_field(this, LANGUAGES, "platform");
    f64::from_bits(nb_str(navigator_platform()).bits())
}

/// `locks`: one plain object stub per instance (typeof === "object"). The Web
/// Locks API is not implemented; the property exists for feature detection.
extern "C" fn get_locks(
    _: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let cached = private_field(this, LOCKS, "locks");
    if !is_undefined(cached) {
        return cached;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(this.as_f64());
    let locks = crate::realm_decl! { birth crate::object::js_object_alloc(0, 0); realm { navigator.locks, kind = namespace, members = { }; } };
    let locks = js_nanbox_pointer(locks as i64);
    crate::object::intrinsic_private_set(receiver.get_nanbox_f64(), LOCKS, locks);
    locks
}

/// `Navigator.prototype`'s getters, in Node's key order (after `constructor`):
/// enumerable, configurable, no setter. A getter that returns a primitive
/// declares its type (not its value: `userAgent` is `Node.js/{NODE_MAJOR}`),
/// which wpfacts reads as the value of a load of the key. Installed by the program's `navigator`
/// install ([`crate::feature_hooks::js_runtime_install_navigator`]) through
/// [`PROTOTYPE_INSTALL`], so a program that never reaches navigator links none of them.
pub(crate) fn install_prototype(proto: *mut crate::object::ObjectHeader) {
    let _gc = crate::gc::GcSuppressScope::new();
    let get = |name: &str, func: *const crate::closure::JsFunctionInfo| {
        crate::event_target::prototype::accessor(proto, name, func, None, true)
    };
    get("hardwareConcurrency", crate::fn_info!(#[realm("Navigator.prototype.hardwareConcurrency.<get>", effects = { returns(prim(num)) })] export crate::navigator::get_hardware_concurrency, 0; with_flags(crate::closure::FN_BUILTIN)));
    get("locks", crate::fn_info!(#[realm("Navigator.prototype.locks.<get>")] export crate::navigator::get_locks, 0; with_flags(crate::closure::FN_BUILTIN)));
    get("language", crate::fn_info!(#[realm("Navigator.prototype.language.<get>", effects = { returns(prim(str)) })] export crate::navigator::get_language, 0; with_flags(crate::closure::FN_BUILTIN)));
    get("languages", crate::fn_info!(#[realm("Navigator.prototype.languages.<get>")] export crate::navigator::get_languages, 0; with_flags(crate::closure::FN_BUILTIN)));
    get("userAgent", crate::fn_info!(#[realm("Navigator.prototype.userAgent.<get>", effects = { returns(prim(str)) })] export crate::navigator::get_user_agent, 0; with_flags(crate::closure::FN_BUILTIN)));
    get("platform", crate::fn_info!(#[realm("Navigator.prototype.platform.<get>", effects = { returns(prim(str)) })] export crate::navigator::get_platform, 0; with_flags(crate::closure::FN_BUILTIN)));
}

pub(crate) static PROTOTYPE_INSTALL: crate::feature_hooks::Hook<
    fn(*mut crate::object::ObjectHeader),
> = crate::feature_hooks::Hook::empty();

/// Realm population's `Navigator.prototype` step: the getters when the
/// program's `navigator` install ran, else only `constructor`.
pub(crate) fn populate_prototype(proto: *mut crate::object::ObjectHeader) {
    if let Some(install) = PROTOTYPE_INSTALL.get() {
        install(proto);
    }
}

#[cfg(test)]
pub(crate) fn test_navigator_object_with_constructor(constructor: f64) -> f64 {
    navigator_object_with_constructor(constructor)
}

#[cfg(test)]
mod tests {
    use super::normalize_navigator_locale;

    /// Restores LC_ALL and LANG when the test ends.
    struct LocaleEnv([(&'static str, Option<std::ffi::OsString>); 2]);

    impl LocaleEnv {
        fn save() -> Self {
            Self([
                ("LC_ALL", std::env::var_os("LC_ALL")),
                ("LANG", std::env::var_os("LANG")),
            ])
        }

        fn set(lc_all: Option<&str>, lang: Option<&str>) {
            for (key, value) in [("LC_ALL", lc_all), ("LANG", lang)] {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    impl Drop for LocaleEnv {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    fn fresh_getter() -> *mut crate::closure::ClosureHeader {
        let getter = crate::closure::js_closure_alloc(
            crate::fn_info!(super::global_this_navigator_getter_thunk, 0),
            1,
        );
        crate::closure::js_closure_set_capture_bits(getter, 0, crate::value::TAG_UNDEFINED);
        getter
    }

    fn read_navigator(getter: *mut crate::closure::ClosureHeader) -> f64 {
        super::global_this_navigator_getter_thunk(getter, crate::closure::JsThis::UNDEFINED)
    }

    fn this(value: f64) -> crate::closure::JsThis {
        crate::closure::JsThis(value.to_bits())
    }

    /// `navigator.language` through its `Navigator.prototype` getter.
    fn language(navigator: f64) -> f64 {
        super::get_language(std::ptr::null(), this(navigator))
    }

    fn string_is(value: f64, expected: &str) -> bool {
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        let expected = super::nb_str(expected);
        crate::string::js_string_equals_value(
            value.get_nanbox_f64(),
            f64::from_bits(expected.bits()),
        ) == 1
    }

    /// The locale is read from LC_ALL, then LANG, when `navigator.language`
    /// is first read, not when the navigator object is built (Node's
    /// `#languages ??=`); later reads answer the kept value, and
    /// `languages` is that one frozen array.
    #[test]
    fn navigator_language_reads_lc_all_then_lang_on_first_language_read() {
        let _global = crate::gc::global_side_table_test_lock();
        let _env = LocaleEnv::save();
        let scope = crate::gc::RuntimeHandleScope::new();

        // The object exists before the environment names the locale.
        LocaleEnv::set(Some("es_ES.UTF-8"), Some("it_IT.UTF-8"));
        let getter = scope.root_raw_mut_ptr(fresh_getter());
        let navigator = scope.root_nanbox_f64(getter.with_mut_ptr(read_navigator));
        LocaleEnv::set(Some("fr_FR.UTF-8"), Some("de_DE.UTF-8"));
        let first = language(navigator.get_nanbox_f64());
        assert!(string_is(first, "fr-FR"));
        // Kept: a later environment change is not seen by this instance.
        LocaleEnv::set(Some("pt_BR.UTF-8"), None);
        assert!(string_is(language(navigator.get_nanbox_f64()), "fr-FR"));
        let languages = scope.root_nanbox_f64(super::get_languages(
            std::ptr::null(),
            this(navigator.get_nanbox_f64()),
        ));
        let again = super::get_languages(std::ptr::null(), this(navigator.get_nanbox_f64()));
        assert_eq!(again.to_bits(), languages.get_nanbox_u64());
        assert_eq!(
            crate::object::js_object_is_frozen(languages.get_nanbox_f64()).to_bits(),
            0x7FFC_0000_0000_0004
        );
        // The same object from the global getter, with its kept value.
        let same = getter.with_mut_ptr(read_navigator);
        assert_eq!(same.to_bits(), navigator.get_nanbox_u64());

        // LC_ALL unset, or C / POSIX: LANG decides.
        LocaleEnv::set(None, Some("de_DE.UTF-8"));
        assert!(string_is(fresh_language(&scope), "de-DE"));
        LocaleEnv::set(Some("C"), Some("pt_BR.UTF-8"));
        assert!(string_is(fresh_language(&scope), "pt-BR"));
        // Neither set: Node's default.
        LocaleEnv::set(None, None);
        assert!(string_is(fresh_language(&scope), "en-US"));
    }

    fn fresh_language(scope: &crate::gc::RuntimeHandleScope) -> f64 {
        let getter = scope.root_raw_mut_ptr(fresh_getter());
        let navigator = scope.root_nanbox_f64(getter.with_mut_ptr(read_navigator));
        language(navigator.get_nanbox_f64())
    }

    /// Building the object leaves every host-derived private field unfilled.
    #[test]
    fn navigator_object_is_born_without_host_values() {
        let _global = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let getter = scope.root_raw_mut_ptr(fresh_getter());
        let navigator = scope.root_nanbox_f64(getter.with_mut_ptr(read_navigator));
        for field in [super::LANGUAGES, super::PARALLELISM, super::LOCKS] {
            let value = crate::object::intrinsic_private_get(navigator.get_nanbox_f64(), field);
            assert_eq!(
                value.map(f64::to_bits),
                Some(crate::value::TAG_UNDEFINED),
                "{field}"
            );
        }
        let cores =
            super::get_hardware_concurrency(std::ptr::null(), this(navigator.get_nanbox_f64()));
        assert!(cores >= 1.0);
        let kept =
            crate::object::intrinsic_private_get(navigator.get_nanbox_f64(), super::PARALLELISM);
        assert_eq!(kept.map(f64::to_bits), Some(cores.to_bits()));
    }

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
