//! Native bindings for the npm `nanoid` package.
//!
//! Functionally identical to `crates/perry-stdlib/src/nanoid.rs`. The
//! point of this crate is that it depends only on [`perry_ffi`], not
//! on `perry-runtime` internals — proving the perry-ffi v0.5 surface
//! still suffices for the second wrapper port (#466 Phase 5 step 2).

use nanoid::nanoid;
use perry_ffi::{alloc_string, read_string, JsString, StringHeader};

/// `nanoid()` — 21-char URL-safe id with the default alphabet.
#[no_mangle]
pub extern "C" fn js_nanoid() -> *mut StringHeader {
    let id = nanoid!();
    alloc_string(&id).as_raw()
}

/// `nanoid(size)` — id with a custom length.
#[no_mangle]
pub extern "C" fn js_nanoid_sized(size: f64) -> *mut StringHeader {
    let size = size as usize;
    if size == 0 {
        return js_nanoid();
    }
    let id = nanoid!(size);
    alloc_string(&id).as_raw()
}

/// `customAlphabet(alphabet, size)()` — id with a user-supplied
/// alphabet. Perry collapses this into a single call rather than the
/// curried form Node uses, so the FFI surface stays flat.
///
/// # Safety
///
/// `alphabet_ptr` must be null or a pointer to a Perry-runtime
/// `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_nanoid_custom(
    alphabet_ptr: *const StringHeader,
    size: f64,
) -> *mut StringHeader {
    let handle = JsString::from_raw(alphabet_ptr as *mut StringHeader);
    let alphabet = match read_string(handle) {
        Some(a) => a,
        None => return js_nanoid(),
    };

    let size = if size <= 0.0 { 21 } else { size as usize };
    let alphabet_chars: Vec<char> = alphabet.chars().collect();

    if alphabet_chars.is_empty() {
        return js_nanoid();
    }

    use rand::Rng;
    let mut rng = rand::rng();
    let id: String = (0..size)
        .map(|_| {
            let idx = rng.random_range(0..alphabet_chars.len());
            alphabet_chars[idx]
        })
        .collect();

    alloc_string(&id).as_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_id_is_21_chars() {
        let handle = unsafe { JsString::from_raw(js_nanoid()) };
        let s = read_string(handle).expect("non-null");
        assert_eq!(s.chars().count(), 21);
    }

    #[test]
    fn sized_id_honors_length() {
        for n in [1, 5, 16, 100] {
            let handle = unsafe { JsString::from_raw(js_nanoid_sized(n as f64)) };
            let s = read_string(handle).expect("non-null");
            assert_eq!(s.chars().count(), n, "size={}", n);
        }
    }

    #[test]
    fn sized_zero_falls_back_to_default() {
        let handle = unsafe { JsString::from_raw(js_nanoid_sized(0.0)) };
        let s = read_string(handle).expect("non-null");
        assert_eq!(s.chars().count(), 21);
    }

    #[test]
    fn custom_alphabet_round_trips_through_perry_ffi() {
        // Allocate the alphabet through perry-ffi so the FFI is
        // exercised end-to-end.
        let alphabet = alloc_string("abc");
        let handle = unsafe { js_nanoid_custom(alphabet.as_raw() as *const _, 8.0) };
        let s = read_string(unsafe { JsString::from_raw(handle) }).expect("non-null");
        assert_eq!(s.chars().count(), 8);
        for c in s.chars() {
            assert!("abc".contains(c), "char `{}` not in alphabet", c);
        }
    }
}
