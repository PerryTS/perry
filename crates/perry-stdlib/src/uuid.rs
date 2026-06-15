//! UUID generation module
//!
//! Native implementation of the 'uuid' npm package using the Rust uuid crate.
//! Supports v1, v4, and v7 UUID generation.

use perry_runtime::{js_string_from_bytes, StringHeader};
use uuid::Uuid;

/// Generate a v4 (random) UUID and return it as a string
/// uuid.v4() -> string
#[no_mangle]
pub extern "C" fn js_uuid_v4() -> *mut StringHeader {
    let uuid = Uuid::new_v4();
    let uuid_str = uuid.to_string();
    js_string_from_bytes(uuid_str.as_ptr(), uuid_str.len() as u32)
}

/// Generate a v1 (timestamp + MAC address) UUID and return it as a string
/// uuid.v1() -> string
/// Note: Uses a random node ID since we don't have access to real MAC
#[no_mangle]
pub extern "C" fn js_uuid_v1() -> *mut StringHeader {
    // v1 requires a timestamp and node ID
    // We use now_v1 which generates based on current time with random node
    let ts = uuid::Timestamp::now(uuid::NoContext);
    let uuid = Uuid::new_v1(ts, &[0x01, 0x23, 0x45, 0x67, 0x89, 0xab]);
    let uuid_str = uuid.to_string();
    js_string_from_bytes(uuid_str.as_ptr(), uuid_str.len() as u32)
}

/// Generate a v7 (Unix timestamp-based) UUID and return it as a string
/// uuid.v7() -> string
#[no_mangle]
pub extern "C" fn js_uuid_v7() -> *mut StringHeader {
    let uuid = Uuid::now_v7();
    let uuid_str = uuid.to_string();
    js_string_from_bytes(uuid_str.as_ptr(), uuid_str.len() as u32)
}

/// Read a `*const StringHeader` into a `&str`, or `""` if null / invalid UTF-8.
unsafe fn read_str<'a>(str_ptr: *const StringHeader) -> &'a str {
    if str_ptr.is_null() {
        return "";
    }
    let len = (*str_ptr).byte_len as usize;
    let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    std::str::from_utf8(bytes).unwrap_or("")
}

/// Parse a namespace argument (string-UUID form) into a `Uuid`, falling
/// back to the nil UUID when the argument isn't a parseable UUID string.
/// The array-namespace form is only reachable via `perry.compilePackages`.
unsafe fn parse_namespace(ns_ptr: *const StringHeader) -> Uuid {
    Uuid::parse_str(read_str(ns_ptr)).unwrap_or_else(|_| Uuid::nil())
}

/// Generate a v5 (SHA-1 name-based) UUID and return it as a string
/// uuid.v5(name, namespace) -> string
#[no_mangle]
pub unsafe extern "C" fn js_uuid_v5(
    name_ptr: *const StringHeader,
    ns_ptr: *const StringHeader,
) -> *mut StringHeader {
    let namespace = parse_namespace(ns_ptr);
    let uuid = Uuid::new_v5(&namespace, read_str(name_ptr).as_bytes());
    let uuid_str = uuid.to_string();
    js_string_from_bytes(uuid_str.as_ptr(), uuid_str.len() as u32)
}

/// Generate a v3 (MD5 name-based) UUID and return it as a string
/// uuid.v3(name, namespace) -> string
#[no_mangle]
pub unsafe extern "C" fn js_uuid_v3(
    name_ptr: *const StringHeader,
    ns_ptr: *const StringHeader,
) -> *mut StringHeader {
    let namespace = parse_namespace(ns_ptr);
    let uuid = Uuid::new_v3(&namespace, read_str(name_ptr).as_bytes());
    let uuid_str = uuid.to_string();
    js_string_from_bytes(uuid_str.as_ptr(), uuid_str.len() as u32)
}

/// Validate if a string is a valid UUID
/// uuid.validate(str) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_uuid_validate(str_ptr: *const StringHeader) -> f64 {
    if str_ptr.is_null() {
        return 0.0; // false
    }

    let len = (*str_ptr).byte_len as usize;
    let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);

    match std::str::from_utf8(bytes) {
        Ok(s) => {
            if Uuid::parse_str(s).is_ok() {
                1.0 // true
            } else {
                0.0 // false
            }
        }
        Err(_) => 0.0, // false
    }
}

/// Get the version of a UUID string
/// uuid.version(str) -> number
#[no_mangle]
pub unsafe extern "C" fn js_uuid_version(str_ptr: *const StringHeader) -> f64 {
    if str_ptr.is_null() {
        return f64::NAN;
    }

    let len = (*str_ptr).byte_len as usize;
    let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);

    match std::str::from_utf8(bytes) {
        Ok(s) => match Uuid::parse_str(s) {
            Ok(uuid) => uuid.get_version_num() as f64,
            Err(_) => f64::NAN,
        },
        Err(_) => f64::NAN,
    }
}

/// Generate a NIL UUID (all zeros)
/// uuid.NIL -> string (constant)
#[no_mangle]
pub extern "C" fn js_uuid_nil() -> *mut StringHeader {
    let uuid_str = Uuid::nil().to_string();
    js_string_from_bytes(uuid_str.as_ptr(), uuid_str.len() as u32)
}
