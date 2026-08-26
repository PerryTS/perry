use super::*;
use std::ffi::{c_char, c_void, CString};

fn test_env() -> NapiEnv {
    crate::gc::ensure_gc_initialized();
    reset_env_for_test();
    current_env()
}

fn int32(env: NapiEnv, value: i32) -> NapiValue {
    let mut result = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_int32(env, value, &mut result) },
        NapiStatus::Ok
    );
    result
}

fn read_int32(env: NapiEnv, value: NapiValue) -> i32 {
    let mut result = 0;
    assert_eq!(
        unsafe { napi_get_value_int32(env, value, &mut result) },
        NapiStatus::Ok
    );
    result
}

#[test]
fn reports_supported_node_api_version() {
    let env = test_env();
    let mut version = 0;
    assert_eq!(
        unsafe { napi_get_version(env, &mut version) },
        NapiStatus::Ok
    );
    assert_eq!(version, NAPI_VERSION);
}

#[test]
fn primitive_values_round_trip_and_report_types() {
    let env = test_env();
    let number = int32(env, -42);
    assert_eq!(read_int32(env, number), -42);

    let mut value_type = NapiValueType::Undefined;
    assert_eq!(
        unsafe { napi_typeof(env, number, &mut value_type) },
        NapiStatus::Ok
    );
    assert_eq!(value_type, NapiValueType::Number);

    let mut boolean = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_get_boolean(env, true, &mut boolean) },
        NapiStatus::Ok
    );
    let mut unboxed = false;
    assert_eq!(
        unsafe { napi_get_value_bool(env, boolean, &mut unboxed) },
        NapiStatus::Ok
    );
    assert!(unboxed);
    assert_eq!(
        unsafe { napi_get_value_double(env, boolean, std::ptr::null_mut()) },
        NapiStatus::InvalidArg
    );
}

#[test]
fn handle_scopes_are_lifo_and_invalidate_local_handles() {
    let env = test_env();
    let mut outer = std::ptr::null_mut();
    let mut inner = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_open_handle_scope(env, &mut outer) },
        NapiStatus::Ok
    );
    let outer_value = int32(env, 1);
    assert_eq!(
        unsafe { napi_open_handle_scope(env, &mut inner) },
        NapiStatus::Ok
    );
    let inner_value = int32(env, 2);

    assert_eq!(
        unsafe { napi_close_handle_scope(env, outer) },
        NapiStatus::HandleScopeMismatch
    );
    assert_eq!(
        unsafe { napi_close_handle_scope(env, inner) },
        NapiStatus::Ok
    );
    let mut ignored = 0;
    assert_eq!(
        unsafe { napi_get_value_int32(env, inner_value, &mut ignored) },
        NapiStatus::InvalidArg
    );
    assert_eq!(read_int32(env, outer_value), 1);
    assert_eq!(
        unsafe { napi_close_handle_scope(env, outer) },
        NapiStatus::Ok
    );

    let slot_count = with_env(env, |env| env.slots.len()).unwrap();
    let mut recycled_scope = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_open_handle_scope(env, &mut recycled_scope) },
        NapiStatus::Ok
    );
    let recycled_value = int32(env, 3);
    assert_eq!(with_env(env, |env| env.slots.len()).unwrap(), slot_count);
    assert_eq!(
        unsafe { napi_get_value_int32(env, inner_value, &mut ignored) },
        NapiStatus::InvalidArg
    );
    assert_eq!(read_int32(env, recycled_value), 3);
    assert_eq!(
        unsafe { napi_close_handle_scope(env, recycled_scope) },
        NapiStatus::Ok
    );
}

#[test]
fn escapable_scope_promotes_exactly_one_handle() {
    let env = test_env();
    let mut scope = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_open_escapable_handle_scope(env, &mut scope) },
        NapiStatus::Ok
    );
    let local = int32(env, 73);
    let mut escaped = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_escape_handle(env, scope, local, &mut escaped) },
        NapiStatus::Ok
    );
    let mut second = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_escape_handle(env, scope, local, &mut second) },
        NapiStatus::EscapeCalledTwice
    );
    assert_eq!(
        unsafe { napi_close_escapable_handle_scope(env, scope) },
        NapiStatus::Ok
    );
    assert_eq!(read_int32(env, escaped), 73);
}

#[test]
fn utf8_latin1_and_utf16_strings_round_trip() {
    let env = test_env();
    let utf8 = CString::new("Perry 🦜").unwrap();
    let mut string = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_string_utf8(env, utf8.as_ptr(), NAPI_AUTO_LENGTH, &mut string) },
        NapiStatus::Ok
    );
    let mut byte_length = 0;
    assert_eq!(
        unsafe {
            napi_get_value_string_utf8(env, string, std::ptr::null_mut(), 0, &mut byte_length)
        },
        NapiStatus::Ok
    );
    let mut bytes = vec![0 as c_char; byte_length + 1];
    let mut copied = 0;
    assert_eq!(
        unsafe {
            napi_get_value_string_utf8(env, string, bytes.as_mut_ptr(), bytes.len(), &mut copied)
        },
        NapiStatus::Ok
    );
    assert_eq!(copied, utf8.as_bytes().len());
    assert_eq!(
        unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<u8>(), copied) },
        utf8.as_bytes()
    );

    let utf16 = [0x0041, 0xd800, 0xd83d, 0xde80];
    let mut wtf16 = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_string_utf16(env, utf16.as_ptr(), utf16.len(), &mut wtf16) },
        NapiStatus::Ok
    );
    let mut out = [0u16; 8];
    let mut units = 0;
    assert_eq!(
        unsafe { napi_get_value_string_utf16(env, wtf16, out.as_mut_ptr(), out.len(), &mut units) },
        NapiStatus::Ok
    );
    assert_eq!(&out[..units], &utf16);

    let latin1 = [0x41u8, 0xe9];
    let mut latin_string = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            napi_create_string_latin1(env, latin1.as_ptr().cast(), latin1.len(), &mut latin_string)
        },
        NapiStatus::Ok
    );
    let mut latin_out = [0 as c_char; 3];
    let mut latin_len = 0;
    assert_eq!(
        unsafe {
            napi_get_value_string_latin1(
                env,
                latin_string,
                latin_out.as_mut_ptr(),
                latin_out.len(),
                &mut latin_len,
            )
        },
        NapiStatus::Ok
    );
    assert_eq!(
        unsafe { std::slice::from_raw_parts(latin_out.as_ptr().cast::<u8>(), latin_len) },
        latin1
    );

    let mut oversized = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            napi_create_string_utf8(env, c"".as_ptr(), i32::MAX as usize + 1, &mut oversized)
        },
        NapiStatus::InvalidArg
    );
    assert_eq!(
        unsafe {
            napi_create_string_utf16(env, [0u16].as_ptr(), i32::MAX as usize + 1, &mut oversized)
        },
        NapiStatus::InvalidArg
    );
}

#[test]
fn objects_arrays_and_named_properties_interoperate() {
    let env = test_env();
    let mut object = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_object(env, &mut object) },
        NapiStatus::Ok
    );
    let value = int32(env, 99);
    assert_eq!(
        unsafe { napi_set_named_property(env, object, c"answer".as_ptr(), value) },
        NapiStatus::Ok
    );
    let mut present = false;
    assert_eq!(
        unsafe { napi_has_named_property(env, object, c"answer".as_ptr(), &mut present) },
        NapiStatus::Ok
    );
    assert!(present);
    let mut read = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_get_named_property(env, object, c"answer".as_ptr(), &mut read) },
        NapiStatus::Ok
    );
    assert_eq!(read_int32(env, read), 99);

    let mut array = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_array_with_length(env, 2, &mut array) },
        NapiStatus::Ok
    );
    assert_eq!(
        unsafe { napi_set_element(env, array, 1, value) },
        NapiStatus::Ok
    );
    let mut length = 0;
    assert_eq!(
        unsafe { napi_get_array_length(env, array, &mut length) },
        NapiStatus::Ok
    );
    assert_eq!(length, 2);
    assert_eq!(
        unsafe { napi_get_element(env, array, 1, &mut read) },
        NapiStatus::Ok
    );
    assert_eq!(read_int32(env, read), 99);
}

#[test]
fn pending_exceptions_and_strong_references_are_roots() {
    let env = test_env();
    let mut scope = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_open_handle_scope(env, &mut scope) },
        NapiStatus::Ok
    );
    let value = int32(env, 17);
    let mut reference = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_reference(env, value, 1, &mut reference) },
        NapiStatus::Ok
    );
    assert_eq!(unsafe { napi_throw(env, value) }, NapiStatus::Ok);
    assert_eq!(
        unsafe { napi_close_handle_scope(env, scope) },
        NapiStatus::Ok
    );

    let mut pending = false;
    assert_eq!(
        unsafe { napi_is_exception_pending(env, &mut pending) },
        NapiStatus::Ok
    );
    assert!(pending);
    let mut exception = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_get_and_clear_last_exception(env, &mut exception) },
        NapiStatus::Ok
    );
    assert_eq!(read_int32(env, exception), 17);

    let mut no_exception = 1usize as NapiValue;
    assert_eq!(
        unsafe { napi_get_and_clear_last_exception(env, &mut no_exception) },
        NapiStatus::Ok
    );
    assert!(no_exception.is_null());

    let mut referenced = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_get_reference_value(env, reference, &mut referenced) },
        NapiStatus::Ok
    );
    assert_eq!(read_int32(env, referenced), 17);
    assert_eq!(
        unsafe { napi_delete_reference(env, reference) },
        NapiStatus::Ok
    );
}

#[test]
fn bigint_date_symbol_and_error_helpers_use_node_api_semantics() {
    let env = test_env();

    let mut bigint = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_bigint_int64(env, -7, &mut bigint) },
        NapiStatus::Ok
    );
    let mut signed = 0;
    let mut lossless = false;
    assert_eq!(
        unsafe { napi_get_value_bigint_int64(env, bigint, &mut signed, &mut lossless) },
        NapiStatus::Ok
    );
    assert_eq!(signed, -7);
    assert!(lossless);
    let mut unsigned = 0;
    assert_eq!(
        unsafe { napi_get_value_bigint_uint64(env, bigint, &mut unsigned, &mut lossless) },
        NapiStatus::Ok
    );
    assert_eq!(unsigned, (-7i64) as u64);
    assert!(!lossless);

    let mut date = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_date(env, 1_234.5, &mut date) },
        NapiStatus::Ok
    );
    let mut is_date = false;
    assert_eq!(
        unsafe { napi_is_date(env, date, &mut is_date) },
        NapiStatus::Ok
    );
    assert!(is_date);
    let mut timestamp = 0.0;
    assert_eq!(
        unsafe { napi_get_date_value(env, date, &mut timestamp) },
        NapiStatus::Ok
    );
    assert_eq!(timestamp, 1_234.5);

    let description = CString::new("identity").unwrap();
    let mut description_value = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            napi_create_string_utf8(
                env,
                description.as_ptr(),
                NAPI_AUTO_LENGTH,
                &mut description_value,
            )
        },
        NapiStatus::Ok
    );
    let mut symbol = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_symbol(env, description_value, &mut symbol) },
        NapiStatus::Ok
    );
    let mut value_type = NapiValueType::Undefined;
    assert_eq!(
        unsafe { napi_typeof(env, symbol, &mut value_type) },
        NapiStatus::Ok
    );
    assert_eq!(value_type, NapiValueType::Symbol);

    assert_eq!(
        unsafe { napi_throw_type_error(env, c"ERR_TEST".as_ptr(), c"boom".as_ptr()) },
        NapiStatus::Ok
    );
    let mut pending = false;
    assert_eq!(
        unsafe { napi_is_exception_pending(env, &mut pending) },
        NapiStatus::Ok
    );
    assert!(pending);
    let mut error = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_get_and_clear_last_exception(env, &mut error) },
        NapiStatus::Ok
    );
    let mut is_error = false;
    assert_eq!(
        unsafe { napi_is_error(env, error, &mut is_error) },
        NapiStatus::Ok
    );
    assert!(is_error);
}

unsafe extern "C" fn add_callback(env: NapiEnv, info: NapiCallbackInfo) -> NapiValue {
    assert_eq!(
        napi_get_cb_info(
            env,
            info,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ),
        NapiStatus::Ok
    );

    let mut padded_argc = 4;
    let mut padded_argv = [std::ptr::null_mut(); 4];
    assert_eq!(
        napi_get_cb_info(
            env,
            info,
            &mut padded_argc,
            padded_argv.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ),
        NapiStatus::Ok
    );
    assert_eq!(padded_argc, 2);
    for value in &padded_argv[2..] {
        let mut value_type = NapiValueType::Object;
        assert_eq!(napi_typeof(env, *value, &mut value_type), NapiStatus::Ok);
        assert_eq!(value_type, NapiValueType::Undefined);
    }

    let mut argc = 2;
    let mut argv = [std::ptr::null_mut(); 2];
    let mut data = std::ptr::null_mut();
    assert_eq!(
        napi_get_cb_info(
            env,
            info,
            &mut argc,
            argv.as_mut_ptr(),
            std::ptr::null_mut(),
            &mut data,
        ),
        NapiStatus::Ok
    );
    assert_eq!(argc, 2);
    assert_eq!(data as usize, 0x8523);
    let sum = read_int32(env, argv[0]) + read_int32(env, argv[1]);
    int32(env, sum)
}

unsafe extern "C" fn throwing_callback(env: NapiEnv, _info: NapiCallbackInfo) -> NapiValue {
    assert_eq!(
        napi_throw_type_error(env, std::ptr::null(), c"callback failed".as_ptr()),
        NapiStatus::Ok
    );
    std::ptr::null_mut()
}

#[test]
fn native_callbacks_receive_arguments_data_and_return_values() {
    let env = test_env();
    let mut function = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            napi_create_function(
                env,
                c"add".as_ptr(),
                NAPI_AUTO_LENGTH,
                Some(add_callback),
                0x8523usize as *mut c_void,
                &mut function,
            )
        },
        NapiStatus::Ok
    );
    let mut value_type = NapiValueType::Undefined;
    assert_eq!(
        unsafe { napi_typeof(env, function, &mut value_type) },
        NapiStatus::Ok
    );
    assert_eq!(value_type, NapiValueType::Function);

    let mut receiver = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_get_undefined(env, &mut receiver) },
        NapiStatus::Ok
    );
    let arguments = [int32(env, 20), int32(env, 22)];
    let mut result = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            napi_call_function(
                env,
                receiver,
                function,
                arguments.len(),
                arguments.as_ptr(),
                &mut result,
            )
        },
        NapiStatus::Ok
    );
    assert_eq!(read_int32(env, result), 42);
}

#[test]
fn native_callback_exceptions_are_caught_before_returning_to_addon_code() {
    let env = test_env();
    let mut function = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            napi_create_function(
                env,
                c"fail".as_ptr(),
                NAPI_AUTO_LENGTH,
                Some(throwing_callback),
                std::ptr::null_mut(),
                &mut function,
            )
        },
        NapiStatus::Ok
    );
    let mut receiver = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_get_undefined(env, &mut receiver) },
        NapiStatus::Ok
    );
    assert_eq!(
        unsafe {
            napi_call_function(
                env,
                receiver,
                function,
                0,
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        },
        NapiStatus::PendingException
    );
    let mut exception = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_get_and_clear_last_exception(env, &mut exception) },
        NapiStatus::Ok
    );
    let mut is_error = false;
    assert_eq!(
        unsafe { napi_is_error(env, exception, &mut is_error) },
        NapiStatus::Ok
    );
    assert!(is_error);
}

#[test]
fn node_api_handles_are_rewritten_by_a_collection() {
    let env = test_env();
    let text = CString::new("a rooted Node-API string that outlives GC").unwrap();
    let mut value = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_string_utf8(env, text.as_ptr(), NAPI_AUTO_LENGTH, &mut value) },
        NapiStatus::Ok
    );
    crate::gc::js_gc_collect();
    let mut length = 0;
    assert_eq!(
        unsafe { napi_get_value_string_utf8(env, value, std::ptr::null_mut(), 0, &mut length) },
        NapiStatus::Ok
    );
    assert_eq!(length, text.as_bytes().len());
}
