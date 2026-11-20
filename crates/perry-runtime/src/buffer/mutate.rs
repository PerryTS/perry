pub(super) fn throw_invalid_buffer_size() -> ! {
    crate::exception::throw_core(crate::error::CoreError::range_error_with_own_code(
        b"Buffer size must be a multiple of the requested word size",
        b"ERR_INVALID_BUFFER_SIZE",
    ))
}

/// Reverse each word without holding a byte borrow across error construction.
fn swap_words(value: f64, width: usize) {
    let result = super::bytes::no_gc(|scope| unsafe {
        let Ok(bytes) = super::bytes::bytes_mut(value, scope) else {
            return true;
        };
        if !bytes.len().is_multiple_of(width) {
            return false;
        }
        for word in bytes.chunks_exact_mut(width) {
            word.reverse();
        }
        true
    });
    if !result {
        throw_invalid_buffer_size();
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_swap16(value: f64) {
    swap_words(value, 2);
}
#[no_mangle]
pub extern "C" fn js_buffer_swap32(value: f64) {
    swap_words(value, 4);
}
#[no_mangle]
pub extern "C" fn js_buffer_swap64(value: f64) {
    swap_words(value, 8);
}
