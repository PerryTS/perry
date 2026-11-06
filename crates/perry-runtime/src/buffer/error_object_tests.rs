//! The errors Buffer methods and tty streams throw for a bad size, argument,
//! range or fd are real error objects (`ErrorHeader`, `GC_TYPE_ERROR`) of
//! the right kind, with the message they always had and an own `code`.

use crate::error::{ErrorHeader, ERROR_KIND_RANGE_ERROR, ERROR_KIND_TYPE_ERROR};
use crate::string::StringHeader;

unsafe fn text(s: *const StringHeader) -> String {
    let len = (*s).byte_len as usize;
    let data = (s as *const u8).add(std::mem::size_of::<StringHeader>());
    String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
}

fn assert_error_object(f: impl FnOnce(), kind: u32, message: &str, code: &str) {
    let caught = crate::exception::catch_thrown(f).expect_err("the helper throws");
    let addr = crate::value::js_nanbox_get_pointer(caught.value()) as usize;
    unsafe {
        assert!(
            crate::error::ptr_is_native_error(addr),
            "not an ErrorHeader"
        );
        let err = addr as *const ErrorHeader;
        assert_eq!((*err).error_kind, kind);
        assert_eq!(text((*err).message), message);
        let own_code = crate::node_submodules::error_user_prop(addr, "code").expect("own code");
        let code_ptr = crate::value::js_nanbox_get_pointer(own_code) as *const StringHeader;
        assert_eq!(text(code_ptr), code);
    }
    assert_eq!(crate::exception::js_has_exception(), 0);
}

#[test]
fn buffer_size_and_range_errors_are_range_errors_with_own_code() {
    assert_error_object(
        || super::from::throw_buffer_alloc_size_out_of_range(),
        ERROR_KIND_RANGE_ERROR,
        "The value of \"size\" is out of range",
        "ERR_OUT_OF_RANGE",
    );
    assert_error_object(
        || super::mutate::throw_invalid_buffer_size(),
        ERROR_KIND_RANGE_ERROR,
        "Buffer size must be a multiple of the requested word size",
        "ERR_INVALID_BUFFER_SIZE",
    );
    assert_error_object(
        || super::numeric::throw_out_of_range(),
        ERROR_KIND_RANGE_ERROR,
        "The value is out of range",
        "ERR_OUT_OF_RANGE",
    );
}

#[test]
fn buffer_concat_bad_element_is_a_type_error_with_own_code() {
    assert_error_object(
        || super::from::throw_buffer_concat_invalid_arg_type(0, 1.0),
        ERROR_KIND_TYPE_ERROR,
        "The \"list[0]\" argument must be an instance of Buffer or Uint8Array. Received type number (1)",
        "ERR_INVALID_ARG_TYPE",
    );
}

#[test]
fn tty_invalid_fd_is_a_range_error_with_own_code() {
    assert_error_object(
        || crate::tty::throw_invalid_fd(-1.0),
        ERROR_KIND_RANGE_ERROR,
        "\"fd\" must be a positive integer: -1",
        "ERR_INVALID_FD",
    );
}
