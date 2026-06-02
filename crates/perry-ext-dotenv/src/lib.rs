//! Native bindings for the npm `dotenv` package.
//!
//! Functionally identical to the implementation that lives in
//! `crates/perry-stdlib/src/dotenv.rs`. The point of this crate is
//! that it depends only on [`perry_ffi`], not on `perry-runtime`
//! internals — proving the perry-ffi v0.5 surface is sufficient for
//! a real wrapper.
//!
//! # Status
//!
//! Additive port (#466 Phase 5 step 1). The original
//! `perry-stdlib::dotenv` stays in place and is what compiled
//! programs link against today. Once a release ships and no
//! regressions surface, the well-known bindings table (#466 Phase 4)
//! flips `import 'dotenv'` resolution to point at this crate, and
//! the old code is deleted.

use perry_ffi::{alloc_string, read_string, JsString, StringHeader};
use std::collections::HashMap;
use std::fs;
use std::sync::Mutex;

static DOTENV_LOADED: Mutex<bool> = Mutex::new(false);

/// Parse a `.env` file's contents into key/value pairs.
///
/// Implementation detail — exposed so the test crate can compare
/// against Node's parsing behavior. Not part of the FFI surface.
fn parse_dotenv_content(content: &str) -> HashMap<String, String> {
    let mut vars = HashMap::new();

    for line in content.lines() {
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].trim().to_string();
            let mut value = line[eq_pos + 1..].trim().to_string();

            if (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''))
            {
                value = value[1..value.len() - 1].to_string();
            }

            if value.contains("\\n") {
                value = value.replace("\\n", "\n");
            }
            if value.contains("\\t") {
                value = value.replace("\\t", "\t");
            }

            vars.insert(key, value);
        }
    }

    vars
}

/// `dotenv.config()` — load `.env` from CWD and apply to `std::env`.
#[no_mangle]
pub extern "C" fn js_dotenv_config() -> f64 {
    // SAFETY: passing a null handle is documented input — the helper
    // below treats it as "use default path .env".
    unsafe { js_dotenv_config_path(std::ptr::null()) }
}

/// `dotenv.config({ path })` — load `.env` from the given path.
///
/// # Safety
///
/// `path_ptr` must be either null (then `.env` is used) or a pointer
/// to a Perry-runtime-allocated `StringHeader`. Caller responsibility
/// is the same contract as any other `extern "C"` function in the
/// stdlib.
#[no_mangle]
pub unsafe extern "C" fn js_dotenv_config_path(path_ptr: *const StringHeader) -> f64 {
    let path = if path_ptr.is_null() {
        ".env".to_string()
    } else {
        let handle = JsString::from_raw(path_ptr as *mut StringHeader);
        read_string(handle)
            .map(|s| s.to_string())
            .unwrap_or_else(|| ".env".to_string())
    };

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return 0.0, // missing file is not an error in dotenv
    };

    let vars = parse_dotenv_content(&content);
    for (key, value) in vars {
        // SAFETY: setting env vars before any thread reads them is
        // the documented use of dotenv. Concurrent set_var from
        // multiple threads is undefined behavior in std — but
        // dotenv.config() runs once at module-init time.
        unsafe { std::env::set_var(&key, &value) };
    }

    *DOTENV_LOADED.lock().unwrap() = true;
    1.0
}

/// `dotenv.parse(content)` — parse `.env`-formatted text into a JSON
/// string the runtime can pass back to TypeScript as an object.
///
/// # Safety
///
/// `content_ptr` must be null or a pointer to a Perry-runtime
/// `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_dotenv_parse(content_ptr: *const StringHeader) -> *mut StringHeader {
    let handle = JsString::from_raw(content_ptr as *mut StringHeader);
    let content = match read_string(handle) {
        Some(c) => c,
        None => return std::ptr::null_mut(),
    };

    let vars = parse_dotenv_content(content);
    let json = serde_json::to_string(&vars).unwrap_or_else(|_| "{}".to_string());
    alloc_string(&json).as_raw()
}

#[cfg(test)]
mod tests {
    use super::parse_dotenv_content;

    #[test]
    fn parses_basic_kv() {
        let vars = parse_dotenv_content("FOO=bar\nBAZ=qux\n");
        assert_eq!(vars.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(vars.get("BAZ"), Some(&"qux".to_string()));
    }

    #[test]
    fn skips_comments_and_empty_lines() {
        let vars = parse_dotenv_content("# comment\n\nFOO=bar\n# another\n");
        assert_eq!(vars.len(), 1);
        assert_eq!(vars.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn unwraps_quoted_values() {
        let vars = parse_dotenv_content(
            r#"DOUBLE="hello"
SINGLE='world'
ESCAPED="line1\nline2"
"#,
        );
        assert_eq!(vars.get("DOUBLE"), Some(&"hello".to_string()));
        assert_eq!(vars.get("SINGLE"), Some(&"world".to_string()));
        assert_eq!(vars.get("ESCAPED"), Some(&"line1\nline2".to_string()));
    }

    #[test]
    fn round_trips_through_perry_ffi() {
        // Allocate a fake .env content string via perry-ffi, run it
        // through js_dotenv_parse, read the JSON back. Proves the
        // wrapper's only contact with the runtime — string read +
        // string alloc — survives end-to-end.
        let content = perry_ffi::alloc_string("KEY=value\n# c\nOTHER=42\n");
        let json_handle = unsafe { super::js_dotenv_parse(content.as_raw() as *const _) };
        let json_handle_wrapped = unsafe { perry_ffi::JsString::from_raw(json_handle) };
        let json_str =
            perry_ffi::read_string(json_handle_wrapped).expect("parse returned non-null");
        // serde_json hash-map order isn't guaranteed; check substrings.
        assert!(json_str.contains("\"KEY\":\"value\""), "got: {}", json_str);
        assert!(json_str.contains("\"OTHER\":\"42\""), "got: {}", json_str);
    }
}
