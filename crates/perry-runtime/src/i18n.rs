//! Internationalization runtime support for Perry.
//!
//! Provides locale detection and a global locale index used by compiled code
//! to select translations from the embedded i18n string table.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

/// Global locale index. 0 = default locale. Set once at startup by perry_i18n_init().
static LOCALE_INDEX: AtomicI32 = AtomicI32::new(0);

/// Whether LOCALE_INDEX has been resolved (via `perry_i18n_init`, an explicit
/// `perry_i18n_set_locale_index`, or the lazy `perry_i18n_locale_index_for`).
/// Guards the lazy path so system-locale detection runs at most once and an
/// explicit set always wins over lazy detection.
static LOCALE_RESOLVED: AtomicBool = AtomicBool::new(false);

/// Returns the current locale index. Called by compiled code to select
/// the correct translation from the i18n string table.
#[no_mangle]
pub extern "C" fn perry_i18n_get_locale_index() -> i32 {
    LOCALE_INDEX.load(Ordering::Relaxed)
}

/// Set the locale index explicitly. Used for dynamic mode locale switching.
#[no_mangle]
pub extern "C" fn perry_i18n_set_locale_index(idx: i32) {
    LOCALE_INDEX.store(idx, Ordering::Relaxed);
    LOCALE_RESOLVED.store(true, Ordering::Release);
}

/// Lazily resolve the locale index for a compiled i18n string table.
///
/// Called by the `Expr::I18nString` lowering at each localized-string site
/// (cheap after the first call). `locales_header` is a StringHeader whose
/// payload is the comma-separated configured locale list in string-table
/// row order (e.g. `"en,de,fr"` — one interned literal per binary);
/// `default_idx` is the configured default locale's row index.
///
/// First call detects the system locale (platform APIs, then env vars) and
/// matches it against the list; no match → `default_idx`. The result is
/// cached in LOCALE_INDEX, so a preceding `perry_i18n_set_locale_index`
/// (dynamic switching) always wins.
#[no_mangle]
pub extern "C" fn perry_i18n_locale_index_for(
    locales_header: *const crate::StringHeader,
    default_idx: i32,
) -> i32 {
    if !LOCALE_RESOLVED.load(Ordering::Acquire) {
        let resolved = (|| -> Option<i32> {
            if locales_header.is_null() {
                return None;
            }
            let payload = unsafe {
                let len = (*locales_header).byte_len as usize;
                let data = locales_header.add(1) as *const u8;
                std::str::from_utf8(std::slice::from_raw_parts(data, len)).ok()?
            };
            let locales: Vec<&str> = payload.split(',').filter(|s| !s.is_empty()).collect();
            if locales.is_empty() {
                return None;
            }
            let system_locale = detect_system_locale()?;
            match_locale(&system_locale, &locales).map(|idx| idx as i32)
        })()
        .unwrap_or(default_idx);
        LOCALE_INDEX.store(resolved, Ordering::Relaxed);
        LOCALE_RESOLVED.store(true, Ordering::Release);
    }
    LOCALE_INDEX.load(Ordering::Relaxed)
}

/// Initialize the i18n system. Detects the system locale and matches it against
/// the configured locale list to set LOCALE_INDEX.
///
/// # Arguments
/// * `locale_codes` - Array of locale code C-string pointers (e.g., "en\0", "de\0")
/// * `locale_lens` - Array of locale code lengths (without null terminator)
/// * `count` - Number of locales
///
/// Called once from the entry module's init function.
#[no_mangle]
pub extern "C" fn perry_i18n_init(
    locale_codes: *const *const u8,
    locale_lens: *const u32,
    count: u32,
) {
    if count == 0 || locale_codes.is_null() || locale_lens.is_null() {
        return;
    }

    // Collect locale codes into strings for matching
    let locales: Vec<&str> = (0..count as usize)
        .filter_map(|i| unsafe {
            let ptr = *locale_codes.add(i);
            let len = *locale_lens.add(i) as usize;
            if ptr.is_null() {
                return None;
            }
            let bytes = std::slice::from_raw_parts(ptr, len);
            std::str::from_utf8(bytes).ok()
        })
        .collect();

    if locales.is_empty() {
        return;
    }

    // Detect system locale
    let system_locale = detect_system_locale();

    let mut log = format!("[i18n] configured locales: {:?}\n", locales);
    log += &format!("[i18n] detected system locale: {:?}\n", system_locale);

    if let Some(locale_str) = system_locale {
        if let Some(idx) = match_locale(&locale_str, &locales) {
            log += &format!("[i18n] matched locale index: {} ({})\n", idx, locales[idx]);
            LOCALE_INDEX.store(idx as i32, Ordering::Relaxed);
        } else {
            log += "[i18n] no match found, using default (index 0)\n";
        }
    } else {
        log += "[i18n] no system locale detected, using default (index 0)\n";
    }
    LOCALE_RESOLVED.store(true, Ordering::Release);
    log += &format!(
        "[i18n] final LOCALE_INDEX: {}\n",
        LOCALE_INDEX.load(Ordering::Relaxed)
    );

    // Write to Documents for retrieval via devicectl
    if let Ok(home) = std::env::var("HOME") {
        let _ = std::fs::write(format!("{}/Documents/i18n-debug.log", home), log.as_bytes());
    }
}

/// Detect the system locale from environment or platform APIs.
fn detect_system_locale() -> Option<String> {
    // 1. Platform-native APIs first (most reliable on GUI apps)
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "watchos",
        target_os = "tvos",
        target_os = "visionos"
    ))]
    {
        if let Some(locale) = detect_apple_locale() {
            return Some(locale);
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(locale) = detect_windows_locale() {
            return Some(locale);
        }
    }

    #[cfg(target_os = "android")]
    {
        if let Some(locale) = detect_android_locale() {
            return Some(locale);
        }
    }

    // 2. Environment variables (Linux, or fallback on other platforms)
    // Try LANGUAGE first (GNU extension, colon-separated list)
    if let Ok(lang) = std::env::var("LANGUAGE") {
        if let Some(first) = lang.split(':').next() {
            if !first.is_empty() {
                return Some(first.to_string());
            }
        }
    }

    // Try LC_ALL, then LC_MESSAGES, then LANG
    for var in &["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(val) = std::env::var(var) {
            if !val.is_empty() && val != "C" && val != "POSIX" {
                return Some(val);
            }
        }
    }

    None
}

/// Match a system locale string against configured locales.
/// Handles formats like "de_DE.UTF-8", "de_DE", "de", "de-DE".
fn match_locale(system_locale: &str, locales: &[&str]) -> Option<usize> {
    // Normalize: strip encoding (e.g., ".UTF-8")
    let locale = system_locale.split('.').next().unwrap_or(system_locale);
    // Normalize separators: "de_DE" -> "de-DE"
    let locale = locale.replace('_', "-");
    let locale_lower = locale.to_lowercase();

    // 1. Exact match (case-insensitive)
    for (i, configured) in locales.iter().enumerate() {
        if configured.to_lowercase() == locale_lower {
            return Some(i);
        }
    }

    // 2. Language-only match: system "de-DE" matches configured "de"
    let lang = locale_lower.split('-').next().unwrap_or(&locale_lower);
    for (i, configured) in locales.iter().enumerate() {
        if configured.to_lowercase() == lang {
            return Some(i);
        }
    }

    // 3. Configured "de-DE" matches system "de" (reverse prefix match)
    for (i, configured) in locales.iter().enumerate() {
        let configured_lang = configured.to_lowercase();
        let configured_base = configured_lang
            .split('-')
            .next()
            .unwrap_or(&configured_lang);
        if configured_base == lang {
            return Some(i);
        }
    }

    None
}

// ============================================================================
// Platform-native locale detection
// ============================================================================

/// Apple platforms (macOS, iOS, watchOS, tvOS, visionOS): use
/// NSBundle.mainBundle.preferredLocalizations to respect per-app language
/// settings in Settings. Falls back to CFLocaleCopyCurrent if NSBundle is
/// unavailable. Both APIs exist on every Apple OS.
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "watchos",
    target_os = "tvos",
    target_os = "visionos"
))]
fn detect_apple_locale() -> Option<String> {
    type CFTypeRef = *const std::ffi::c_void;
    type CFStringRef = CFTypeRef;
    type CFIndex = isize;

    extern "C" {
        fn CFStringGetLength(string: CFStringRef) -> CFIndex;
        fn CFStringGetCString(
            string: CFStringRef,
            buffer: *mut u8,
            buffer_size: CFIndex,
            encoding: u32,
        ) -> bool;
        fn CFRelease(cf: CFTypeRef);
        fn CFLocaleCopyCurrent() -> CFTypeRef;
        fn CFLocaleGetIdentifier(locale: CFTypeRef) -> CFStringRef;
    }

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x08000100;

    // Helper to convert CFString/NSString to Rust String
    unsafe fn cfstring_to_string(s: CFStringRef) -> Option<String> {
        if s.is_null() {
            return None;
        }
        let len = CFStringGetLength(s);
        if len <= 0 || len > 64 {
            return None;
        }
        let mut buf = [0u8; 128];
        let ok = CFStringGetCString(
            s,
            buf.as_mut_ptr(),
            buf.len() as CFIndex,
            K_CF_STRING_ENCODING_UTF8,
        );
        if ok {
            std::ffi::CStr::from_ptr(buf.as_ptr() as *const i8)
                .to_str()
                .ok()
                .map(|s| s.to_string())
        } else {
            None
        }
    }

    // Try NSBundle.mainBundle.preferredLocalizations.firstObject first
    // This respects per-app language settings on iOS
    unsafe {
        extern "C" {
            fn objc_getClass(name: *const i8) -> *const std::ffi::c_void;
            fn sel_registerName(name: *const i8) -> *const std::ffi::c_void;
            fn objc_msgSend(
                receiver: *const std::ffi::c_void,
                sel: *const std::ffi::c_void,
                ...
            ) -> *const std::ffi::c_void;
        }

        let ns_bundle = objc_getClass(b"NSBundle\0".as_ptr() as *const i8);
        if !ns_bundle.is_null() {
            let main_bundle_sel = sel_registerName(b"mainBundle\0".as_ptr() as *const i8);
            let bundle = objc_msgSend(ns_bundle, main_bundle_sel);
            if !bundle.is_null() {
                let pref_sel = sel_registerName(b"preferredLocalizations\0".as_ptr() as *const i8);
                let localizations = objc_msgSend(bundle, pref_sel);
                if !localizations.is_null() {
                    let first_sel = sel_registerName(b"firstObject\0".as_ptr() as *const i8);
                    let first = objc_msgSend(localizations, first_sel);
                    if let Some(lang) = cfstring_to_string(first as CFStringRef) {
                        return Some(lang);
                    }
                }
            }
        }
    }

    // Fallback: CFLocaleCopyCurrent
    unsafe {
        let locale = CFLocaleCopyCurrent();
        if locale.is_null() {
            return None;
        }
        let identifier = CFLocaleGetIdentifier(locale);
        let result = cfstring_to_string(identifier);
        CFRelease(locale);
        result
    }
}

/// Windows: use GetUserDefaultLocaleName (Win32 API).
#[cfg(target_os = "windows")]
fn detect_windows_locale() -> Option<String> {
    extern "system" {
        fn GetUserDefaultLocaleName(lp_locale_name: *mut u16, cch_locale_name: i32) -> i32;
    }

    let mut buf = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH = 85
    let len = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };

    if len > 0 {
        let name = String::from_utf16_lossy(&buf[..len as usize - 1]); // strip null
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

/// Android: read persist.sys.locale or ro.product.locale system properties.
#[cfg(target_os = "android")]
fn detect_android_locale() -> Option<String> {
    // __system_property_get is available in Android's libc (bionic)
    extern "C" {
        fn __system_property_get(name: *const u8, value: *mut u8) -> i32;
    }

    let props: &[&[u8]] = &[
        b"persist.sys.locale\0",
        b"ro.product.locale\0",
        b"persist.sys.language\0",
    ];

    for prop in props {
        let mut buf = [0u8; 92]; // PROP_VALUE_MAX = 92
        let len = unsafe { __system_property_get(prop.as_ptr(), buf.as_mut_ptr()) };
        if len > 0 {
            if let Ok(s) = std::str::from_utf8(&buf[..len as usize]) {
                let s = s.trim_end_matches('\0');
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

// --- String interpolation ---

/// Substitute `{param}` placeholders in a template string with provided values.
///
/// # Arguments
/// * `template_ptr` - Pointer to the template StringHeader (e.g., "Hello, {name}!")
/// * `param_names` - Array of param name StringHeader pointers
/// * `param_values` - Array of param value StringHeader pointers (already stringified)
/// * `param_count` - Number of parameters
///
/// # Returns
/// Pointer to a new StringHeader with all `{param}` substituted.
#[no_mangle]
pub extern "C" fn perry_i18n_interpolate(
    template_ptr: *mut crate::StringHeader,
    param_names: *const *mut crate::StringHeader,
    param_values: *const *mut crate::StringHeader,
    param_count: u32,
) -> *mut crate::StringHeader {
    if template_ptr.is_null() {
        return crate::string::js_string_from_bytes(std::ptr::null(), 0);
    }

    // Get the raw bytes of the template
    let template_bytes = unsafe {
        let len = (*template_ptr).byte_len as usize;
        let data_ptr = template_ptr.add(1) as *const u8;
        std::slice::from_raw_parts(data_ptr, len)
    };

    // Collect param names and values
    let mut replacements: Vec<(&str, &[u8])> = Vec::with_capacity(param_count as usize);
    for i in 0..param_count as usize {
        unsafe {
            let name_ptr = *param_names.add(i);
            let value_ptr = *param_values.add(i);
            if name_ptr.is_null() || value_ptr.is_null() {
                continue;
            }
            let name_len = (*name_ptr).byte_len as usize;
            let name_data = name_ptr.add(1) as *const u8;
            let name =
                std::str::from_utf8_unchecked(std::slice::from_raw_parts(name_data, name_len));

            let value_len = (*value_ptr).byte_len as usize;
            let value_data = value_ptr.add(1) as *const u8;
            let value_bytes = std::slice::from_raw_parts(value_data, value_len);

            replacements.push((name, value_bytes));
        }
    }

    // Build result by scanning for {param} patterns
    let mut result = Vec::with_capacity(template_bytes.len() + 64);
    let mut i = 0;
    let bytes = template_bytes;

    while i < bytes.len() {
        if bytes[i] == b'{' {
            // Look for closing brace
            if let Some(close) = bytes[i + 1..].iter().position(|&b| b == b'}') {
                let param_name =
                    unsafe { std::str::from_utf8_unchecked(&bytes[i + 1..i + 1 + close]) };
                // Find matching replacement
                let mut found = false;
                for (name, value) in &replacements {
                    if *name == param_name {
                        result.extend_from_slice(value);
                        found = true;
                        break;
                    }
                }
                if found {
                    i += 2 + close; // skip {param}
                    continue;
                }
            }
        }
        result.push(bytes[i]);
        i += 1;
    }

    crate::string::js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

// --- Plural rules (CLDR) ---

/// Returns the CLDR plural category for a given count in a given locale.
/// Categories: 0=zero, 1=one, 2=two, 3=few, 4=many, 5=other
///
/// Based on CLDR plural rules v44.
#[no_mangle]
pub extern "C" fn perry_i18n_plural_category(locale_idx: i32, count: f64) -> i32 {
    // The locale_idx maps to the locale code set during init.
    // We use a thread-local to store the locale codes for lookup.
    let category = PLURAL_LOCALE_CODES.with(|codes| {
        let codes = codes.borrow();
        let locale = codes
            .get(locale_idx as usize)
            .map(|s| s.as_str())
            .unwrap_or("en");
        cldr_plural_category(locale, count)
    });
    category as i32
}

/// Initialize the plural rule locale code list.
/// Called from codegen init alongside perry_i18n_init.
#[no_mangle]
pub extern "C" fn perry_i18n_set_plural_locales(
    locale_codes: *const *const u8,
    locale_lens: *const u32,
    count: u32,
) {
    let locales: Vec<String> = (0..count as usize)
        .filter_map(|i| unsafe {
            let ptr = *locale_codes.add(i);
            let len = *locale_lens.add(i) as usize;
            if ptr.is_null() {
                return None;
            }
            let bytes = std::slice::from_raw_parts(ptr, len);
            std::str::from_utf8(bytes).ok().map(|s| s.to_string())
        })
        .collect();
    PLURAL_LOCALE_CODES.with(|codes| {
        *codes.borrow_mut() = locales;
    });
}

use std::cell::RefCell;
thread_local! {
    static PLURAL_LOCALE_CODES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// CLDR plural category constants
const PLURAL_ZERO: u8 = 0;
const PLURAL_ONE: u8 = 1;
const PLURAL_TWO: u8 = 2;
const PLURAL_FEW: u8 = 3;
const PLURAL_MANY: u8 = 4;
const PLURAL_OTHER: u8 = 5;

/// Hand-rolled CLDR plural rules for common locales.
/// Based on https://www.unicode.org/cldr/charts/44/supplemental/language_plural_rules.html
fn cldr_plural_category(locale: &str, n: f64) -> u8 {
    // Extract base language from locale code (e.g., "de-DE" -> "de")
    let lang = locale.split(&['-', '_'][..]).next().unwrap_or(locale);

    // Integer and fraction detection
    let abs_n = n.abs();
    let is_int = n.fract() == 0.0 && abs_n < 1e15;
    let i = if is_int { abs_n as u64 } else { 0 };

    match lang {
        // Germanic/Romance: one=1, other
        "en" | "de" | "nl" | "sv" | "da" | "no" | "nb" | "nn" | "fi" | "et" | "hu" | "tr"
        | "el" | "he" | "it" | "es" | "pt" | "ca" | "gl" | "eu" | "bg" | "hi" | "bn" | "gu"
        | "kn" | "ml" | "mr" | "ta" | "te" | "ur" | "sw" => {
            if is_int && i == 1 {
                PLURAL_ONE
            } else {
                PLURAL_OTHER
            }
        }

        // French/Brazilian Portuguese: one=0..1, other
        "fr" => {
            if is_int && i <= 1 {
                PLURAL_ONE
            } else {
                PLURAL_OTHER
            }
        }

        // East Asian: no plural distinction
        "ja" | "zh" | "ko" | "vi" | "th" | "lo" | "my" | "km" | "id" | "ms" => PLURAL_OTHER,

        // Slavic: Russian, Ukrainian, Serbian, Croatian, Bosnian
        // one: i%10=1 and i%100!=11
        // few: i%10=2..4 and i%100!=12..14
        // many: i%10=0 or i%10=5..9 or i%100=11..14
        "ru" | "uk" | "sr" | "hr" | "bs" => {
            if !is_int {
                return PLURAL_OTHER;
            }
            let i10 = i % 10;
            let i100 = i % 100;
            if i10 == 1 && i100 != 11 {
                PLURAL_ONE
            } else if (2..=4).contains(&i10) && !(12..=14).contains(&i100) {
                PLURAL_FEW
            } else {
                PLURAL_MANY
            }
        }

        // Polish: one=1, few: i%10=2..4 and i%100!=12..14, many/other
        "pl" => {
            if !is_int {
                return PLURAL_OTHER;
            }
            if i == 1 {
                return PLURAL_ONE;
            }
            let i10 = i % 10;
            let i100 = i % 100;
            if (2..=4).contains(&i10) && !(12..=14).contains(&i100) {
                PLURAL_FEW
            } else {
                PLURAL_MANY
            }
        }

        // Czech, Slovak
        "cs" | "sk" => {
            if !is_int {
                return PLURAL_MANY;
            }
            if i == 1 {
                PLURAL_ONE
            } else if (2..=4).contains(&i) {
                PLURAL_FEW
            } else {
                PLURAL_OTHER
            }
        }

        // Arabic: zero=0, one=1, two=2, few=3..10, many=11..99, other
        "ar" => {
            if !is_int {
                return PLURAL_OTHER;
            }
            match i {
                0 => PLURAL_ZERO,
                1 => PLURAL_ONE,
                2 => PLURAL_TWO,
                _ => {
                    let i100 = i % 100;
                    if (3..=10).contains(&i100) {
                        PLURAL_FEW
                    } else if (11..=99).contains(&i100) {
                        PLURAL_MANY
                    } else {
                        PLURAL_OTHER
                    }
                }
            }
        }

        // Romanian: one=1, few=0 or n%100=2..19
        "ro" => {
            if !is_int {
                return PLURAL_OTHER;
            }
            if i == 1 {
                PLURAL_ONE
            } else if i == 0 || (2..=19).contains(&(i % 100)) {
                PLURAL_FEW
            } else {
                PLURAL_OTHER
            }
        }

        // Lithuanian: one=i%10==1 and i%100!=11..19, few=i%10=2..9 and i%100!=12..19
        "lt" => {
            if !is_int {
                return PLURAL_OTHER;
            }
            let i10 = i % 10;
            let i100 = i % 100;
            if i10 == 1 && !(11..=19).contains(&i100) {
                PLURAL_ONE
            } else if (2..=9).contains(&i10) && !(12..=19).contains(&i100) {
                PLURAL_FEW
            } else {
                PLURAL_OTHER
            }
        }

        // Latvian: zero=0, one=i%10==1 and i%100!=11
        "lv" => {
            if !is_int {
                return PLURAL_OTHER;
            }
            if i == 0 {
                PLURAL_ZERO
            } else if i % 10 == 1 && i % 100 != 11 {
                PLURAL_ONE
            } else {
                PLURAL_OTHER
            }
        }

        // Default: simple one/other
        _ => {
            if is_int && i == 1 {
                PLURAL_ONE
            } else {
                PLURAL_OTHER
            }
        }
    }
}

// ============================================================================
// Locale-aware formatting functions
// ============================================================================

/// Locale formatting data: (decimal_sep, thousands_sep, currency_symbol, currency_after, percent_space)
struct LocaleFormat {
    decimal: &'static str,
    thousands: &'static str,
    currency_symbol: &'static str,
    currency_after: bool,  // true = "23,10 €", false = "$23.10"
    percent_space: bool,   // true = "42 %", false = "42%"
    date_order: DateOrder, // MDY, DMY, YMD
    time_24h: bool,
}

#[derive(Clone, Copy)]
enum DateOrder {
    MDY,
    DMY,
    YMD,
}

fn locale_format(locale: &str) -> LocaleFormat {
    let lang = locale.split(&['-', '_'][..]).next().unwrap_or(locale);
    match lang {
        "en" => LocaleFormat {
            decimal: ".",
            thousands: ",",
            currency_symbol: "$",
            currency_after: false,
            percent_space: false,
            date_order: DateOrder::MDY,
            time_24h: false,
        },
        "de" | "nl" | "tr" => LocaleFormat {
            decimal: ",",
            thousands: ".",
            currency_symbol: "€",
            currency_after: true,
            percent_space: true,
            date_order: DateOrder::DMY,
            time_24h: true,
        },
        "fr" => LocaleFormat {
            decimal: ",",
            thousands: "\u{202f}",
            currency_symbol: "€",
            currency_after: true,
            percent_space: true,
            date_order: DateOrder::DMY,
            time_24h: true,
        },
        "es" | "it" | "pt" | "ca" | "gl" | "ro" => LocaleFormat {
            decimal: ",",
            thousands: ".",
            currency_symbol: "€",
            currency_after: true,
            percent_space: true,
            date_order: DateOrder::DMY,
            time_24h: true,
        },
        "ja" => LocaleFormat {
            decimal: ".",
            thousands: ",",
            currency_symbol: "¥",
            currency_after: false,
            percent_space: false,
            date_order: DateOrder::YMD,
            time_24h: true,
        },
        "zh" => LocaleFormat {
            decimal: ".",
            thousands: ",",
            currency_symbol: "¥",
            currency_after: false,
            percent_space: false,
            date_order: DateOrder::YMD,
            time_24h: true,
        },
        "ko" => LocaleFormat {
            decimal: ".",
            thousands: ",",
            currency_symbol: "₩",
            currency_after: false,
            percent_space: false,
            date_order: DateOrder::YMD,
            time_24h: true,
        },
        "ru" | "uk" => LocaleFormat {
            decimal: ",",
            thousands: "\u{00a0}",
            currency_symbol: "₽",
            currency_after: true,
            percent_space: true,
            date_order: DateOrder::DMY,
            time_24h: true,
        },
        "pl" | "cs" | "sk" | "hr" | "sr" | "bs" => LocaleFormat {
            decimal: ",",
            thousands: "\u{00a0}",
            currency_symbol: "zł",
            currency_after: true,
            percent_space: true,
            date_order: DateOrder::DMY,
            time_24h: true,
        },
        "ar" | "he" => LocaleFormat {
            decimal: ".",
            thousands: ",",
            currency_symbol: "$",
            currency_after: true,
            percent_space: true,
            date_order: DateOrder::DMY,
            time_24h: true,
        },
        "sv" | "da" | "no" | "nb" | "nn" | "fi" => LocaleFormat {
            decimal: ",",
            thousands: "\u{00a0}",
            currency_symbol: "kr",
            currency_after: true,
            percent_space: true,
            date_order: DateOrder::YMD,
            time_24h: true,
        },
        _ => LocaleFormat {
            decimal: ".",
            thousands: ",",
            currency_symbol: "$",
            currency_after: false,
            percent_space: false,
            date_order: DateOrder::MDY,
            time_24h: false,
        },
    }
}

fn get_locale_code(locale_idx: i32) -> String {
    PLURAL_LOCALE_CODES.with(|codes| {
        let codes = codes.borrow();
        codes
            .get(locale_idx as usize)
            .cloned()
            .unwrap_or_else(|| "en".to_string())
    })
}

/// Format a number with locale-appropriate grouping and decimal separator.
fn format_number_locale(value: f64, fmt: &LocaleFormat) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }

    let is_negative = value.is_sign_negative();

    // Use Ryū via `{}` for minimum round-trip digits (matches JS semantics).
    let s = format!("{}", value);
    let (int_digits, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i.trim_start_matches('-').to_string(), Some(f.to_string())),
        None => (s.trim_start_matches('-').to_string(), None),
    };

    // Apply thousands grouping to the integer digits.
    let mut grouped = String::with_capacity(int_digits.len() + int_digits.len() / 3 + 4);
    for (i, ch) in int_digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            grouped.insert_str(0, fmt.thousands);
        }
        grouped.insert(0, ch);
    }

    let result = match frac_part {
        Some(f) => format!("{}{}{}", grouped, fmt.decimal, f),
        None => grouped,
    };

    if is_negative {
        format!("-{}", result)
    } else {
        result
    }
}

/// Format a number with locale grouping.
/// perry_i18n_format_number(value: f64, locale_idx: i32) -> *mut StringHeader
#[no_mangle]
pub extern "C" fn perry_i18n_format_number(
    value: f64,
    locale_idx: i32,
) -> *mut crate::StringHeader {
    let locale = get_locale_code(locale_idx);
    let fmt = locale_format(&locale);
    let result = format_number_locale(value, &fmt);
    let bytes = result.as_bytes();
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Format a number as currency.
/// perry_i18n_format_currency(value: f64, locale_idx: i32) -> *mut StringHeader
#[no_mangle]
pub extern "C" fn perry_i18n_format_currency(
    value: f64,
    locale_idx: i32,
) -> *mut crate::StringHeader {
    let locale = get_locale_code(locale_idx);
    let fmt = locale_format(&locale);

    let is_negative = value < 0.0;
    let abs = value.abs();

    // Always 2 decimal places for currency
    let int_part = abs.trunc() as u64;
    let frac_part = ((abs.fract() * 100.0).round() as u64) % 100;

    // Format integer with thousands grouping
    let int_str = int_part.to_string();
    let mut grouped = String::with_capacity(int_str.len() + 8);
    for (i, ch) in int_str.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            grouped.insert_str(0, fmt.thousands);
        }
        grouped.insert(0, ch);
    }

    let number_str = format!("{}{}{:02}", grouped, fmt.decimal, frac_part);

    let result = if fmt.currency_after {
        format!(
            "{}{}\u{00a0}{}",
            if is_negative { "-" } else { "" },
            number_str,
            fmt.currency_symbol
        )
    } else {
        format!(
            "{}{}{}",
            if is_negative { "-" } else { "" },
            fmt.currency_symbol,
            number_str
        )
    };

    let bytes = result.as_bytes();
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Format a number as percentage.
/// perry_i18n_format_percent(value: f64, locale_idx: i32) -> *mut StringHeader
#[no_mangle]
pub extern "C" fn perry_i18n_format_percent(
    value: f64,
    locale_idx: i32,
) -> *mut crate::StringHeader {
    let locale = get_locale_code(locale_idx);
    let fmt = locale_format(&locale);
    let pct = value * 100.0;
    let pct_rounded = (pct * 100.0).round() / 100.0; // 2 decimal max

    let pct_str = if pct_rounded.fract() == 0.0 {
        format!("{}", pct_rounded as i64)
    } else {
        let s = format!("{:.2}", pct_rounded);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    // Replace decimal separator
    let pct_str = pct_str.replace('.', fmt.decimal);

    let result = if fmt.percent_space {
        format!("{}\u{00a0}%", pct_str)
    } else {
        format!("{}%", pct_str)
    };

    let bytes = result.as_bytes();
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Month names for date formatting (English, used as fallback)
const MONTH_NAMES_EN: &[&str] = &[
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTH_NAMES_DE: &[&str] = &[
    "Januar",
    "Februar",
    "März",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
];
const MONTH_NAMES_FR: &[&str] = &[
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];
const MONTH_NAMES_ES: &[&str] = &[
    "enero",
    "febrero",
    "marzo",
    "abril",
    "mayo",
    "junio",
    "julio",
    "agosto",
    "septiembre",
    "octubre",
    "noviembre",
    "diciembre",
];

fn month_names(lang: &str) -> &'static [&'static str] {
    match lang {
        "de" => MONTH_NAMES_DE,
        "fr" => MONTH_NAMES_FR,
        "es" => MONTH_NAMES_ES,
        _ => MONTH_NAMES_EN,
    }
}

/// Format a date (timestamp in ms since epoch).
/// style: 0=medium (default), 1=short, 2=long
/// perry_i18n_format_date(timestamp: f64, style: i32, locale_idx: i32) -> *mut StringHeader
#[no_mangle]
pub extern "C" fn perry_i18n_format_date(
    timestamp: f64,
    style: i32,
    locale_idx: i32,
) -> *mut crate::StringHeader {
    let locale = get_locale_code(locale_idx);
    let lang = locale.split(&['-', '_'][..]).next().unwrap_or(&locale);
    let fmt = locale_format(&locale);

    // Convert timestamp to components
    let ts_secs = (timestamp / 1000.0) as i64;
    let (year, month, day, _, _, _) = crate::date::timestamp_to_components(ts_secs);
    let month_idx = (month as usize).saturating_sub(1).min(11);

    let result = match style {
        1 => {
            // Short: "3/22/2026" or "22.03.2026" or "2026/03/22"
            match fmt.date_order {
                DateOrder::MDY => format!("{}/{}/{}", month, day, year),
                DateOrder::DMY => format!("{:02}.{:02}.{}", day, month, year),
                DateOrder::YMD => format!("{}/{:02}/{:02}", year, month, day),
            }
        }
        2 => {
            // Long: "Sunday, March 22, 2026" or "Sonntag, 22. März 2026"
            let month_name = month_names(lang)[month_idx];
            match fmt.date_order {
                DateOrder::MDY => format!("{} {}, {}", month_name, day, year),
                DateOrder::DMY => format!("{}. {} {}", day, month_name, year),
                DateOrder::YMD => format!("{} {} {}", year, month_name, day),
            }
        }
        _ => {
            // Medium (default): "March 22, 2026" or "22. März 2026"
            let month_name = month_names(lang)[month_idx];
            match fmt.date_order {
                DateOrder::MDY => format!("{} {}, {}", month_name, day, year),
                DateOrder::DMY => format!("{}. {} {}", day, month_name, year),
                DateOrder::YMD => format!("{} {}月 {}", year, month_name, day),
            }
        }
    };

    let bytes = result.as_bytes();
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Format a time (timestamp in ms since epoch).
/// perry_i18n_format_time(timestamp: f64, locale_idx: i32) -> *mut StringHeader
#[no_mangle]
pub extern "C" fn perry_i18n_format_time(
    timestamp: f64,
    locale_idx: i32,
) -> *mut crate::StringHeader {
    let locale = get_locale_code(locale_idx);
    let fmt = locale_format(&locale);

    let ts_secs = (timestamp / 1000.0) as i64;
    let (_, _, _, hour, minute, _) = crate::date::timestamp_to_components(ts_secs);

    let result = if fmt.time_24h {
        format!("{:02}:{:02}", hour, minute)
    } else {
        let (h12, ampm) = if hour == 0 {
            (12, "AM")
        } else if hour < 12 {
            (hour, "AM")
        } else if hour == 12 {
            (12, "PM")
        } else {
            (hour - 12, "PM")
        };
        format!("{}:{:02} {}", h12, minute, ampm)
    };

    let bytes = result.as_bytes();
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

// =============================================================================
// Default-locale single-arg wrappers
// =============================================================================
//
// The TS façade in `types/perry/i18n/index.d.ts` declares each format function
// as `(value: number) => string` — no locale arg. The two-arg runtime entries
// above keep the explicit-locale form available for callers that want it
// (mainly internal use), and these wrappers fold in the active locale via
// `LOCALE_INDEX` so the codegen dispatch table in `lower_call.rs` can map
// each TS name to one runtime symbol with a single `f64` arg.

#[no_mangle]
pub extern "C" fn perry_i18n_format_number_default(value: f64) -> *mut crate::StringHeader {
    perry_i18n_format_number(value, LOCALE_INDEX.load(Ordering::Relaxed))
}

#[no_mangle]
pub extern "C" fn perry_i18n_format_currency_default(value: f64) -> *mut crate::StringHeader {
    perry_i18n_format_currency(value, LOCALE_INDEX.load(Ordering::Relaxed))
}

#[no_mangle]
pub extern "C" fn perry_i18n_format_percent_default(value: f64) -> *mut crate::StringHeader {
    perry_i18n_format_percent(value, LOCALE_INDEX.load(Ordering::Relaxed))
}

#[no_mangle]
pub extern "C" fn perry_i18n_format_date_short(timestamp: f64) -> *mut crate::StringHeader {
    perry_i18n_format_date(timestamp, 1, LOCALE_INDEX.load(Ordering::Relaxed))
}

#[no_mangle]
pub extern "C" fn perry_i18n_format_date_long(timestamp: f64) -> *mut crate::StringHeader {
    perry_i18n_format_date(timestamp, 2, LOCALE_INDEX.load(Ordering::Relaxed))
}

#[no_mangle]
pub extern "C" fn perry_i18n_format_time_default(timestamp: f64) -> *mut crate::StringHeader {
    perry_i18n_format_time(timestamp, LOCALE_INDEX.load(Ordering::Relaxed))
}

/// `Raw(value)` — pass-through for `Text("...{x}", { x: Raw(v) })` patterns
/// where the parameter name might otherwise trigger automatic formatting.
/// Always returns the value's plain string form, regardless of locale.
#[no_mangle]
pub extern "C" fn perry_i18n_format_raw(value: f64) -> *mut crate::StringHeader {
    crate::value::js_jsvalue_to_string(value)
}
