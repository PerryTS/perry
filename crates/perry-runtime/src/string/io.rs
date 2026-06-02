//! stdout / stderr printing helpers (`console.log`, `console.error`,
//! `console.warn` lowering targets).

use super::*;

/// Print a string to stdout
#[no_mangle]
pub extern "C" fn js_string_print(s: *const StringHeader) {
    if !is_valid_string_ptr(s) {
        println!();
        return;
    }

    let str_data = string_as_str(s);
    println!("{}", str_data);
}

/// Print a string to stderr (console.error)
#[no_mangle]
pub extern "C" fn js_string_error(s: *const StringHeader) {
    if !is_valid_string_ptr(s) {
        eprintln!();
        return;
    }

    let str_data = string_as_str(s);
    eprintln!("{}", str_data);
}

/// Print a string to stderr (console.warn)
#[no_mangle]
pub extern "C" fn js_string_warn(s: *const StringHeader) {
    if !is_valid_string_ptr(s) {
        eprintln!();
        return;
    }

    let str_data = string_as_str(s);
    eprintln!("{}", str_data);
}
