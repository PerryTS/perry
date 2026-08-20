//! Dotenv module (dotenv compatible)
//!
//! Native implementation of the 'dotenv' npm package.
//! Loads environment variables from .env files.

use perry_runtime::{js_string_from_bytes, StringHeader};
use std::collections::HashMap;
use std::fs;
use std::sync::Mutex;

use crate::common::string_from_header;

lazy_static::lazy_static! {
    static ref DOTENV_LOADED: Mutex<bool> = Mutex::new(false);
}

/// Parse a .env file content into key-value pairs
fn parse_dotenv_content(content: &str) -> HashMap<String, String> {
    let mut vars = HashMap::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Find the first '=' to split key and value
        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].trim().to_string();
            let mut value = line[eq_pos + 1..].trim().to_string();

            // Remove surrounding quotes if present
            if (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''))
            {
                value = value[1..value.len() - 1].to_string();
            }

            // Handle escape sequences in double-quoted strings
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

/// Load .env file and set environment variables
/// dotenv.config() -> void
#[no_mangle]
pub extern "C" fn js_dotenv_config() -> f64 {
    // SAFETY: We're passing a null pointer which is handled safely by js_dotenv_config_path
    unsafe { js_dotenv_config_path(std::ptr::null()) }
}

/// Load .env file from a specific path
/// dotenv.config({ path: '.env.local' }) -> void
#[no_mangle]
pub unsafe extern "C" fn js_dotenv_config_path(path_ptr: *const StringHeader) -> f64 {
    let path = if path_ptr.is_null() {
        ".env".to_string()
    } else {
        string_from_header(path_ptr).unwrap_or_else(|| ".env".to_string())
    };

    // Read the file
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return 0.0, // File not found is not an error in dotenv
    };

    // Parse and set environment variables
    let vars = parse_dotenv_content(&content);
    for (key, value) in vars {
        std::env::set_var(&key, &value);
    }

    *DOTENV_LOADED.lock().unwrap() = true;
    1.0 // Success
}

/// Parse a string as dotenv format without setting env vars
/// dotenv.parse(content) -> object
#[no_mangle]
pub unsafe extern "C" fn js_dotenv_parse(content_ptr: *const StringHeader) -> *mut StringHeader {
    let content = match string_from_header(content_ptr) {
        Some(c) => c,
        None => return std::ptr::null_mut(),
    };

    let vars = parse_dotenv_content(&content);

    // Return as JSON string (simple key-value object)
    let json = serde_json::to_string(&vars).unwrap_or_else(|_| "{}".to_string());
    js_string_from_bytes(json.as_ptr(), json.len() as u32)
}
