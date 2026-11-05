use super::*;
use std::ffi::{c_char, c_void, CString};
use std::sync::atomic::{AtomicUsize, Ordering};

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
fn last_error_info_tracks_each_status_and_message() {
    let env = test_env();
    let read = || {
        let mut info = std::ptr::null();
        assert_eq!(
            unsafe { napi_get_last_error_info(env, &mut info) },
            NapiStatus::Ok
        );
        let info = unsafe { &*info };
        let message = unsafe { std::ffi::CStr::from_ptr(info.error_message) };
        (info.error_code, message.to_str().unwrap().to_string())
    };
    let number = int32(env, 1);
    assert_eq!(read(), (NapiStatus::Ok, "napi_ok".to_string()));
    let mut ignored = false;
    assert_eq!(
        unsafe { napi_get_value_bool(env, number, &mut ignored) },
        NapiStatus::BooleanExpected
    );
    assert_eq!(
        read(),
        (
            NapiStatus::BooleanExpected,
            "value must be a boolean".to_string()
        )
    );
    let mut string = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_coerce_to_string(env, number, &mut string) },
        NapiStatus::Ok
    );
    assert_eq!(read(), (NapiStatus::Ok, "napi_ok".to_string()));
    assert_eq!(
        unsafe { napi_get_value_double(env, number, std::ptr::null_mut()) },
        NapiStatus::InvalidArg
    );
    assert_eq!(
        read(),
        (
            NapiStatus::InvalidArg,
            "result must not be null".to_string()
        )
    );
    assert_eq!(
        unsafe { napi_get_value_bool(env, std::ptr::null_mut(), &mut ignored) },
        NapiStatus::InvalidArg
    );
    assert_eq!(
        read(),
        (
            NapiStatus::InvalidArg,
            "value is not a live handle".to_string()
        )
    );
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
fn handle_scopes_are_lifo_and_release_local_handles() {
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
    // A closed scope releases its slots; a released handle keeps reading
    // until a new handle reuses its slot (V8 moves only the block top).
    assert_eq!(read_int32(env, inner_value), 2);
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
    let recycled_values = [int32(env, 3), int32(env, 4)];
    assert_eq!(with_env(env, |env| env.slots.len()).unwrap(), slot_count);
    let mut ignored = 0;
    for released in [inner_value, outer_value] {
        assert_eq!(
            unsafe { napi_get_value_int32(env, released, &mut ignored) },
            NapiStatus::InvalidArg
        );
    }
    assert_eq!(read_int32(env, recycled_values[0]), 3);
    assert_eq!(read_int32(env, recycled_values[1]), 4);
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
            napi_create_string_utf16(
                env,
                [0u16].as_ptr(),
                u32::MAX as usize / 3 + 1,
                &mut oversized,
            )
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

/// A Node-API named store is a named key add (`o.port = v`), not a computed
/// one: objects an addon builds with the same names in the same order follow
/// the same shape transitions, so they share one ShapeId and none of them
/// grows an overflow. Only the first object's adds mint shapes. A name that
/// already has its atom is looked up, not materialized: reading it back
/// allocates nothing.
#[test]
fn named_stores_of_addon_built_objects_share_one_shape() {
    let env = test_env();
    let names = [c"msg", c"length", c"offset", c"port", c"address"];
    let mut first_mints = 0;
    let mut per_add = Vec::new();
    let mut shared = None;
    for i in 0..100_000 {
        let mut scope = std::ptr::null_mut();
        assert_eq!(
            unsafe { napi_open_handle_scope(env, &mut scope) },
            NapiStatus::Ok
        );
        let mut object = std::ptr::null_mut();
        assert_eq!(
            unsafe { napi_create_object(env, &mut object) },
            NapiStatus::Ok
        );
        if i == 0 {
            first_mints = crate::object::shapes::test_shape_id_counter();
        }
        for (slot, name) in names.iter().enumerate() {
            let value = int32(env, i + slot as i32);
            assert_eq!(
                unsafe { napi_set_named_property(env, object, name.as_ptr(), value) },
                NapiStatus::Ok
            );
            if i == 0 {
                per_add.push(crate::object::shapes::test_shape_id_counter() - first_mints);
            }
        }
        let obj = (value_bits(env, object).expect("live object") & crate::value::POINTER_MASK)
            as *const crate::object::ObjectHeader;
        assert!(
            !unsafe { crate::object::overflow::has_overflow(obj) },
            "a named store must not append to the overflow (object {i})"
        );
        let stamp = unsafe { crate::object::shapes::object_shape_stamp(obj) };
        match shared {
            None => {
                let minted = crate::object::shapes::test_shape_id_counter() - first_mints;
                // The first object pays the layout's mints once (its empty
                // birth layout and the add edges); the count is bounded by
                // the names, never by the number of objects.
                assert!(
                    minted as usize <= 2 * names.len(),
                    "the first object's five adds mint a bounded set of shapes, \
                     minted {minted} (after each add: {per_add:?})"
                );
                shared = Some((stamp, crate::object::shapes::test_shape_id_counter()));
            }
            Some((shape, _)) => assert_eq!(
                stamp, shape,
                "object {i} must carry the first object's ShapeId"
            ),
        }
        if i == 99_999 {
            let mut read = std::ptr::null_mut();
            let mut present = false;
            let before = crate::arena::arena_in_use_bytes();
            for _ in 0..1_000 {
                assert_eq!(
                    unsafe { napi_get_named_property(env, object, c"port".as_ptr(), &mut read) },
                    NapiStatus::Ok
                );
                assert_eq!(
                    unsafe {
                        napi_has_named_property(env, object, c"offset".as_ptr(), &mut present)
                    },
                    NapiStatus::Ok
                );
            }
            assert_eq!(
                crate::arena::arena_in_use_bytes(),
                before,
                "a named read of an existing name allocates no key string"
            );
            assert_eq!(read_int32(env, read), i + 3);
            assert!(present);
        }
        assert_eq!(
            unsafe { napi_close_handle_scope(env, scope) },
            NapiStatus::Ok
        );
    }
    let (_, mints_after_first) = shared.expect("objects were built");
    assert_eq!(
        crate::object::shapes::test_shape_id_counter(),
        mints_after_first,
        "objects after the first follow the cached transitions and mint no ShapeId"
    );
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

/// node-addon-api's `Napi::HandleScope scope(env)` shape: the callback
/// builds its result inside its own scope, the scope closes, then the
/// callback returns the result.
unsafe extern "C" fn own_scope_object_callback(env: NapiEnv, _info: NapiCallbackInfo) -> NapiValue {
    let mut scope = std::ptr::null_mut();
    assert_eq!(napi_open_handle_scope(env, &mut scope), NapiStatus::Ok);
    let mut object = std::ptr::null_mut();
    assert_eq!(napi_create_object(env, &mut object), NapiStatus::Ok);
    assert_eq!(
        napi_set_named_property(env, object, c"master".as_ptr(), int32(env, 31)),
        NapiStatus::Ok
    );
    assert_eq!(napi_close_handle_scope(env, scope), NapiStatus::Ok);
    object
}

/// Returns a handle whose slot a later handle took.
unsafe extern "C" fn reused_handle_callback(env: NapiEnv, _info: NapiCallbackInfo) -> NapiValue {
    let mut scope = std::ptr::null_mut();
    assert_eq!(napi_open_handle_scope(env, &mut scope), NapiStatus::Ok);
    let stale = int32(env, 5);
    assert_eq!(napi_close_handle_scope(env, scope), NapiStatus::Ok);
    let _reuses_the_slot = int32(env, 6);
    stale
}

fn call_native(
    env: NapiEnv,
    name: &std::ffi::CStr,
    callback: NapiCallback,
) -> (NapiStatus, NapiValue) {
    let mut function = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            napi_create_function(
                env,
                name.as_ptr(),
                NAPI_AUTO_LENGTH,
                callback,
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
    let mut result = std::ptr::null_mut();
    let status =
        unsafe { napi_call_function(env, receiver, function, 0, std::ptr::null(), &mut result) };
    (status, result)
}

#[test]
fn a_callback_returns_the_object_it_built_in_its_own_closed_scope() {
    let env = test_env();
    let (status, object) = call_native(env, c"openScoped", Some(own_scope_object_callback));
    assert_eq!(status, NapiStatus::Ok);
    let mut value_type = NapiValueType::Undefined;
    assert_eq!(
        unsafe { napi_typeof(env, object, &mut value_type) },
        NapiStatus::Ok
    );
    assert_eq!(value_type, NapiValueType::Object);
    let mut master = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_get_named_property(env, object, c"master".as_ptr(), &mut master) },
        NapiStatus::Ok
    );
    assert_eq!(read_int32(env, master), 31);
}

#[test]
fn a_returned_handle_whose_slot_was_reused_throws() {
    let env = test_env();
    let (status, _) = call_native(env, c"reused", Some(reused_handle_callback));
    assert_eq!(status, NapiStatus::PendingException);
    let mut exception = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_get_and_clear_last_exception(env, &mut exception) },
        NapiStatus::Ok
    );
    let mut code = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_get_named_property(env, exception, c"code".as_ptr(), &mut code) },
        NapiStatus::Ok
    );
    let mut buffer = [0u8; 64];
    let mut length = 0;
    assert_eq!(
        unsafe {
            napi_get_value_string_utf8(
                env,
                code,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut length,
            )
        },
        NapiStatus::Ok
    );
    assert_eq!(&buffer[..length], b"ERR_INVALID_RETURN_VALUE");
}

#[test]
fn a_collection_ends_a_released_handle() {
    let env = test_env();
    let mut scope = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_open_handle_scope(env, &mut scope) },
        NapiStatus::Ok
    );
    let mut object = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_object(env, &mut object) },
        NapiStatus::Ok
    );
    assert_eq!(
        unsafe { napi_close_handle_scope(env, scope) },
        NapiStatus::Ok
    );
    let mut value_type = NapiValueType::Undefined;
    assert_eq!(
        unsafe { napi_typeof(env, object, &mut value_type) },
        NapiStatus::Ok
    );
    // A released slot is no root; after a collection its handle must not
    // read a value the collector may have moved or freed.
    crate::gc::js_gc_collect();
    assert_eq!(
        unsafe { napi_typeof(env, object, &mut value_type) },
        NapiStatus::InvalidArg
    );
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

#[test]
fn descriptors_property_names_and_bigint_words_round_trip() {
    let env = test_env();
    let mut object = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_object(env, &mut object) },
        NapiStatus::Ok
    );
    let answer = int32(env, 42);
    let descriptor = NapiPropertyDescriptor {
        utf8name: c"answer".as_ptr(),
        name: std::ptr::null_mut(),
        method: None,
        getter: None,
        setter: None,
        value: answer,
        attributes: NAPI_WRITABLE | NAPI_ENUMERABLE | NAPI_CONFIGURABLE,
        data: std::ptr::null_mut(),
    };
    assert_eq!(
        unsafe { napi_define_properties(env, object, 1, &descriptor) },
        NapiStatus::Ok
    );
    let mut names = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            napi_get_all_property_names(
                env,
                object,
                NapiKeyCollectionMode::OwnOnly,
                NAPI_KEY_ENUMERABLE | NAPI_KEY_SKIP_SYMBOLS,
                NapiKeyConversion::NumbersToStrings,
                &mut names,
            )
        },
        NapiStatus::Ok
    );
    let mut name_count = 0;
    assert_eq!(
        unsafe { napi_get_array_length(env, names, &mut name_count) },
        NapiStatus::Ok
    );
    assert_eq!(name_count, 1);

    let mut words = vec![0u64; 33];
    words[0] = 0x0123_4567_89ab_cdef;
    words[32] = 0xfedc_ba98_7654_3210;
    let mut bigint = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_bigint_words(env, 0, words.len(), words.as_ptr(), &mut bigint) },
        NapiStatus::Ok
    );
    let mut sign = -1;
    let mut count = 0;
    assert_eq!(
        unsafe {
            napi_get_value_bigint_words(
                env,
                bigint,
                std::ptr::null_mut(),
                &mut count,
                std::ptr::null_mut(),
            )
        },
        NapiStatus::Ok
    );
    assert_eq!(count, words.len());
    let mut output = vec![0u64; words.len()];
    assert_eq!(
        unsafe {
            napi_get_value_bigint_words(env, bigint, &mut sign, &mut count, output.as_mut_ptr())
        },
        NapiStatus::Ok
    );
    assert_eq!(output, words);
}

#[test]
fn buffers_views_detach_and_promises_keep_backing_identity() {
    let env = test_env();
    let mut arraybuffer = std::ptr::null_mut();
    let mut bytes = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_arraybuffer(env, 16, &mut bytes, &mut arraybuffer) },
        NapiStatus::Ok
    );
    assert!(!bytes.is_null());
    let mut typed = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            napi_create_typedarray(
                env,
                NapiTypedarrayType::Uint32Array as i32,
                4,
                arraybuffer,
                0,
                &mut typed,
            )
        },
        NapiStatus::Ok
    );
    let mut kind = NapiTypedarrayType::Int8Array;
    let mut length = 0;
    let mut data = std::ptr::null_mut();
    let mut backing = std::ptr::null_mut();
    let mut offset = usize::MAX;
    assert_eq!(
        unsafe {
            napi_get_typedarray_info(
                env,
                typed,
                &mut kind,
                &mut length,
                &mut data,
                &mut backing,
                &mut offset,
            )
        },
        NapiStatus::Ok
    );
    assert_eq!(kind, NapiTypedarrayType::Uint32Array);
    assert_eq!((length, offset), (4, 0));
    assert_eq!(data, bytes);
    assert!(!backing.is_null());

    assert_eq!(
        unsafe { napi_detach_arraybuffer(env, arraybuffer) },
        NapiStatus::Ok
    );
    let mut detached = false;
    assert_eq!(
        unsafe { napi_is_detached_arraybuffer(env, arraybuffer, &mut detached) },
        NapiStatus::Ok
    );
    assert!(detached);

    let mut promise = std::ptr::null_mut();
    let mut deferred = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_promise(env, &mut deferred, &mut promise) },
        NapiStatus::Ok
    );
    let mut is_promise = false;
    assert_eq!(
        unsafe { napi_is_promise(env, promise, &mut is_promise) },
        NapiStatus::Ok
    );
    assert!(is_promise);
    let resolved = int32(env, 7);
    assert_eq!(
        unsafe { napi_resolve_deferred(env, deferred, resolved) },
        NapiStatus::Ok
    );
}

#[test]
fn buffer_backed_uint8arrays_report_typedarray_info() {
    let env = test_env();

    let inspect = |value: NapiValue,
                   expected_length: usize,
                   expected_offset: usize,
                   expected_data: *mut c_void,
                   expected_backing: Option<NapiValue>| {
        let mut is_typed_array = false;
        assert_eq!(
            unsafe { napi_is_typedarray(env, value, &mut is_typed_array) },
            NapiStatus::Ok
        );
        assert!(is_typed_array);

        let mut kind = NapiTypedarrayType::Int8Array;
        let mut length = 0;
        let mut data = std::ptr::null_mut();
        let mut backing = std::ptr::null_mut();
        let mut offset = usize::MAX;
        assert_eq!(
            unsafe {
                napi_get_typedarray_info(
                    env,
                    value,
                    &mut kind,
                    &mut length,
                    &mut data,
                    &mut backing,
                    &mut offset,
                )
            },
            NapiStatus::Ok
        );
        assert_eq!(kind, NapiTypedarrayType::Uint8Array);
        assert_eq!((length, offset), (expected_length, expected_offset));
        assert_eq!(data, expected_data);

        let mut is_array_buffer = false;
        assert_eq!(
            unsafe { napi_is_arraybuffer(env, backing, &mut is_array_buffer) },
            NapiStatus::Ok
        );
        assert!(is_array_buffer);
        if let Some(expected) = expected_backing {
            let mut same = false;
            assert_eq!(
                unsafe { napi_strict_equals(env, backing, expected, &mut same) },
                NapiStatus::Ok
            );
            assert!(same);
        }

        let mut backing_data = std::ptr::null_mut();
        let mut backing_length = 0;
        assert_eq!(
            unsafe {
                napi_get_arraybuffer_info(env, backing, &mut backing_data, &mut backing_length)
            },
            NapiStatus::Ok
        );
        assert!(backing_length >= expected_offset + expected_length);
        assert_eq!(
            data,
            unsafe { (backing_data as *mut u8).add(expected_offset) }.cast()
        );

        let mut backing_again = std::ptr::null_mut();
        assert_eq!(
            unsafe {
                napi_get_typedarray_info(
                    env,
                    value,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut backing_again,
                    std::ptr::null_mut(),
                )
            },
            NapiStatus::Ok
        );
        let mut same = false;
        assert_eq!(
            unsafe { napi_strict_equals(env, backing, backing_again, &mut same) },
            NapiStatus::Ok
        );
        assert!(
            same,
            "the reported backing ArrayBuffer identity must be stable"
        );
    };

    let mut arraybuffer = std::ptr::null_mut();
    let mut arraybuffer_data = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_arraybuffer(env, 8, &mut arraybuffer_data, &mut arraybuffer) },
        NapiStatus::Ok
    );
    let arraybuffer_bits = value_bits(env, arraybuffer).unwrap();
    let view_ptr = crate::buffer::js_uint8array_view(f64::from_bits(arraybuffer_bits), 2.0, 3.0);
    let view = add_handle(env, crate::value::JSValue::pointer(view_ptr.cast()).bits()).unwrap();
    inspect(
        view,
        3,
        2,
        unsafe { (arraybuffer_data as *mut u8).add(2) }.cast(),
        Some(arraybuffer),
    );

    let mut node_buffer = std::ptr::null_mut();
    let mut node_buffer_data = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_buffer(env, 4, &mut node_buffer_data, &mut node_buffer) },
        NapiStatus::Ok
    );
    inspect(node_buffer, 4, 0, node_buffer_data, None);

    let mut is_typed_array = true;
    assert_eq!(
        unsafe { napi_is_typedarray(env, arraybuffer, &mut is_typed_array) },
        NapiStatus::Ok
    );
    assert!(!is_typed_array, "an ArrayBuffer is not a TypedArray");
}

/// napi_get_typedarray_info answers type, length, data and byte offset from
/// the view's own header. A caller that does not ask for the ArrayBuffer
/// (node-addon-api's `TypedArrayOf`, the quiche stream read path) gets no
/// ArrayBuffer object materialized: 1000 fresh Uint8Arrays and Uint16Arrays
/// asked for everything but the buffer allocate nothing and keep no bag.
#[test]
fn typedarray_info_without_the_arraybuffer_reads_only_the_header() {
    use crate::typedarray::KIND_UINT16;
    let env = test_env();
    let mut arrays = Vec::new();
    for i in 0..1_000u32 {
        let (addr, kind, len) = if i % 2 == 0 {
            let buffer = crate::buffer::js_uint8array_alloc(8);
            (buffer as usize, NapiTypedarrayType::Uint8Array, 8)
        } else {
            let typed = crate::byte_cell_support::typed_array_alloc(KIND_UINT16, 6);
            (typed as usize, NapiTypedarrayType::Uint16Array, 6)
        };
        let handle = add_handle(
            env,
            crate::value::JSValue::pointer(addr as *const u8).bits(),
        )
        .unwrap();
        arrays.push((addr, handle, kind, len));
    }
    let before = crate::arena::arena_in_use_bytes();
    for &(addr, handle, expected_kind, expected_len) in &arrays {
        let mut kind = NapiTypedarrayType::Int8Array;
        let mut length = 0;
        let mut data = std::ptr::null_mut();
        let mut offset = usize::MAX;
        assert_eq!(
            unsafe {
                napi_get_typedarray_info(
                    env,
                    handle,
                    &mut kind,
                    &mut length,
                    &mut data,
                    std::ptr::null_mut(),
                    &mut offset,
                )
            },
            NapiStatus::Ok
        );
        assert_eq!((kind, length, offset), (expected_kind, expected_len, 0));
        assert_eq!(data, unsafe { crate::buffer::store::data(addr) }.cast());
        assert!(
            unsafe { crate::buffer::store::bag(addr) }.is_null(),
            "no ArrayBuffer alias is minted for a caller that did not ask for it"
        );
    }
    assert_eq!(
        crate::arena::arena_in_use_bytes(),
        before,
        "typed-array info without the ArrayBuffer allocates nothing"
    );
}

static ASYNC_EXECUTED: AtomicUsize = AtomicUsize::new(0);
static ASYNC_COMPLETED: AtomicUsize = AtomicUsize::new(0);
static ASYNC_OFF_THREAD_REJECTED: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn async_execute(env: NapiEnv, _data: *mut c_void) {
    ASYNC_EXECUTED.fetch_add(1, Ordering::SeqCst);
    let mut value = std::ptr::null_mut();
    if napi_get_undefined(env, &mut value) == NapiStatus::InvalidArg {
        ASYNC_OFF_THREAD_REJECTED.fetch_add(1, Ordering::SeqCst);
    }
}

unsafe extern "C" fn async_complete(_env: NapiEnv, status: NapiStatus, _data: *mut c_void) {
    assert_eq!(status, NapiStatus::Ok);
    ASYNC_COMPLETED.fetch_add(1, Ordering::SeqCst);
}

#[test]
fn async_work_executes_off_thread_and_completes_on_owner() {
    ASYNC_EXECUTED.store(0, Ordering::SeqCst);
    ASYNC_COMPLETED.store(0, Ordering::SeqCst);
    ASYNC_OFF_THREAD_REJECTED.store(0, Ordering::SeqCst);
    // turnloop P4: the work item goes to the shared blocking pool instead of a
    // fresh thread per queue. "Off thread" alone cannot tell the two apart —
    // both satisfy every assertion below — so the pool's own counter is read
    // around the queue. A run that fell back to the thread path leaves it
    // unchanged, and this says so instead of passing quietly.
    #[cfg(not(target_arch = "wasm32"))]
    let pool_before = crate::turnloop_pool::submitted_total();
    let env = test_env();
    let mut name = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_string_utf8(env, c"work".as_ptr(), 4, &mut name) },
        NapiStatus::Ok
    );
    let mut work = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            napi_create_async_work(
                env,
                std::ptr::null_mut(),
                name,
                Some(async_execute),
                Some(async_complete),
                std::ptr::null_mut(),
                &mut work,
            )
        },
        NapiStatus::Ok
    );
    assert_eq!(unsafe { napi_queue_async_work(env, work) }, NapiStatus::Ok);
    for _ in 0..200 {
        process_pending();
        if ASYNC_COMPLETED.load(Ordering::SeqCst) != 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(ASYNC_EXECUTED.load(Ordering::SeqCst), 1);
    assert_eq!(ASYNC_OFF_THREAD_REJECTED.load(Ordering::SeqCst), 1);
    assert_eq!(ASYNC_COMPLETED.load(Ordering::SeqCst), 1);
    #[cfg(not(target_arch = "wasm32"))]
    assert_eq!(
        crate::turnloop_pool::submitted_total(),
        pool_before + 1,
        "the work item went to the shared blocking pool, not to a fresh thread"
    );
    assert_eq!(unsafe { napi_delete_async_work(env, work) }, NapiStatus::Ok);
}

static TSFN_CALLED: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn tsfn_call_js(
    env: NapiEnv,
    _function: NapiValue,
    context: *mut c_void,
    data: *mut c_void,
) {
    assert!(!env.is_null());
    assert_eq!(context as usize, 0x8523);
    TSFN_CALLED.fetch_add(data as usize, Ordering::SeqCst);
}

#[test]
fn threadsafe_function_accepts_foreign_thread_calls_and_drains_on_owner() {
    TSFN_CALLED.store(0, Ordering::SeqCst);
    let env = test_env();
    let mut name = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_string_utf8(env, c"tsfn".as_ptr(), 4, &mut name) },
        NapiStatus::Ok
    );
    let mut tsfn = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            napi_create_threadsafe_function(
                env,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                name,
                1,
                1,
                std::ptr::null_mut(),
                None,
                0x8523usize as *mut c_void,
                Some(tsfn_call_js),
                &mut tsfn,
            )
        },
        NapiStatus::Ok
    );
    let token = tsfn as usize;
    std::thread::spawn(move || unsafe {
        let tsfn = token as NapiThreadsafeFunction;
        assert_eq!(
            napi_call_threadsafe_function(
                tsfn,
                3usize as *mut c_void,
                NapiThreadsafeFunctionCallMode::Nonblocking,
            ),
            NapiStatus::Ok
        );
        assert_eq!(
            napi_release_threadsafe_function(tsfn, NapiThreadsafeFunctionReleaseMode::Release),
            NapiStatus::Ok
        );
    })
    .join()
    .unwrap();
    process_pending();
    assert_eq!(TSFN_CALLED.load(Ordering::SeqCst), 3);
}

static LIFECYCLE_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
static ASYNC_CLEANUP_STATUS: AtomicUsize = AtomicUsize::new(usize::MAX);
static INSTANCE_FINALIZER_DATA: AtomicUsize = AtomicUsize::new(0);
static INSTANCE_FINALIZER_HINT: AtomicUsize = AtomicUsize::new(0);

fn record_lifecycle_step(step: usize) {
    LIFECYCLE_SEQUENCE
        .try_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
            Some(value.saturating_mul(10).saturating_add(step))
        })
        .unwrap();
}

unsafe extern "C" fn async_cleanup(handle: NapiAsyncCleanupHookHandle, argument: *mut c_void) {
    record_lifecycle_step(argument as usize);
    ASYNC_CLEANUP_STATUS.store(
        napi_remove_async_cleanup_hook(handle) as usize,
        Ordering::SeqCst,
    );
}

unsafe extern "C" fn cleanup(argument: *mut c_void) {
    record_lifecycle_step(argument as usize);
}

unsafe extern "C" fn instance_finalizer(_env: NapiEnv, data: *mut c_void, hint: *mut c_void) {
    INSTANCE_FINALIZER_DATA.store(data as usize, Ordering::SeqCst);
    INSTANCE_FINALIZER_HINT.store(hint as usize, Ordering::SeqCst);
    record_lifecycle_step(4);
}

#[test]
fn instance_data_and_cleanup_hooks_run_once_in_shutdown_order() {
    LIFECYCLE_SEQUENCE.store(0, Ordering::SeqCst);
    ASYNC_CLEANUP_STATUS.store(usize::MAX, Ordering::SeqCst);
    INSTANCE_FINALIZER_DATA.store(0, Ordering::SeqCst);
    INSTANCE_FINALIZER_HINT.store(0, Ordering::SeqCst);

    let env = test_env();
    assert_eq!(
        unsafe {
            napi_set_instance_data(
                env,
                0x8523usize as *mut c_void,
                Some(instance_finalizer),
                0x1234usize as *mut c_void,
            )
        },
        NapiStatus::Ok
    );
    let mut instance_data = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_get_instance_data(env, &mut instance_data) },
        NapiStatus::Ok
    );
    assert_eq!(instance_data as usize, 0x8523);

    // Node runs one cleanup queue, newest first, whatever the hook kind.
    assert_eq!(
        unsafe { napi_add_env_cleanup_hook(env, Some(cleanup), 2usize as *mut c_void) },
        NapiStatus::Ok
    );
    let mut async_handle = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            napi_add_async_cleanup_hook(
                env,
                Some(async_cleanup),
                1usize as *mut c_void,
                &mut async_handle,
            )
        },
        NapiStatus::Ok
    );
    assert_eq!(
        unsafe { napi_add_env_cleanup_hook(env, Some(cleanup), 3usize as *mut c_void) },
        NapiStatus::Ok
    );

    shutdown_current_env();
    assert_eq!(LIFECYCLE_SEQUENCE.load(Ordering::SeqCst), 3124);
    assert_eq!(
        ASYNC_CLEANUP_STATUS.load(Ordering::SeqCst),
        NapiStatus::Ok as usize
    );
    assert_eq!(INSTANCE_FINALIZER_DATA.load(Ordering::SeqCst), 0x8523);
    assert_eq!(INSTANCE_FINALIZER_HINT.load(Ordering::SeqCst), 0x1234);

    shutdown_current_env();
    assert_eq!(LIFECYCLE_SEQUENCE.load(Ordering::SeqCst), 3124);
}

#[test]
fn async_cleanup_hook_removal_handle_is_optional() {
    LIFECYCLE_SEQUENCE.store(0, Ordering::SeqCst);
    ASYNC_CLEANUP_STATUS.store(usize::MAX, Ordering::SeqCst);
    let env = test_env();
    // Node accepts a null `remove_handle`; the hook still receives its handle.
    assert_eq!(
        unsafe {
            napi_add_async_cleanup_hook(
                env,
                Some(async_cleanup),
                5usize as *mut c_void,
                std::ptr::null_mut(),
            )
        },
        NapiStatus::Ok
    );
    shutdown_current_env();
    assert_eq!(LIFECYCLE_SEQUENCE.load(Ordering::SeqCst), 5);
    assert_eq!(
        ASYNC_CLEANUP_STATUS.load(Ordering::SeqCst),
        NapiStatus::Ok as usize
    );
}

#[test]
fn run_script_and_uv_loop_fail_explicitly() {
    let env = test_env();
    let mut script = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_string_utf8(env, c"1+1".as_ptr(), 3, &mut script) },
        NapiStatus::Ok
    );
    let mut result = 1usize as NapiValue;
    assert_eq!(
        unsafe { napi_run_script(env, script, &mut result) },
        NapiStatus::GenericFailure
    );
    assert!(result.is_null());
    let mut loop_pointer = 1usize as *mut c_void;
    assert_eq!(
        unsafe { napi_get_uv_event_loop(env, &mut loop_pointer) },
        NapiStatus::GenericFailure
    );
    assert!(loop_pointer.is_null());
}

#[test]
fn external_buffer_finalizer_runs_once_after_the_owner_dies() {
    unsafe extern "C" fn release(_env: NapiEnv, data: *mut c_void, hint: *mut c_void) {
        drop(Box::from_raw(data as *mut [u8; 3]));
        (*(hint as *const AtomicUsize)).fetch_add(1, Ordering::SeqCst);
    }
    let env = test_env();
    let releases = AtomicUsize::new(0);
    let data = Box::into_raw(Box::new([65u8, 66, 67]));
    let mut scope = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_open_handle_scope(env, &mut scope) },
        NapiStatus::Ok
    );
    let mut value = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            napi_create_external_buffer(
                env,
                3,
                data.cast(),
                Some(release),
                (&releases as *const AtomicUsize).cast_mut().cast(),
                &mut value,
            )
        },
        NapiStatus::Ok
    );
    let owner = (value_bits(env, value).unwrap() & crate::value::POINTER_MASK) as usize;
    assert!(crate::buffer::is_foreign_backed_buffer(owner));
    crate::gc::js_gc_collect();
    super::metadata::drain_pending_finalizers();
    assert_eq!(
        releases.load(Ordering::SeqCst),
        0,
        "live owner must retain bytes"
    );
    assert_eq!(
        unsafe { napi_close_handle_scope(env, scope) },
        NapiStatus::Ok
    );
    crate::gc::js_gc_collect();
    super::metadata::drain_pending_finalizers();
    assert_eq!(
        releases.load(Ordering::SeqCst),
        1,
        "dead owner's native resource must be released"
    );
    crate::gc::js_gc_collect();
    super::metadata::drain_pending_finalizers();
    assert_eq!(
        releases.load(Ordering::SeqCst),
        1,
        "finalizer entry must be removed"
    );
    let live_data = Box::into_raw(Box::new([68u8, 69, 70]));
    assert_eq!(
        unsafe {
            napi_create_external_buffer(
                env,
                3,
                live_data.cast(),
                Some(release),
                (&releases as *const AtomicUsize).cast_mut().cast(),
                &mut value,
            )
        },
        NapiStatus::Ok
    );
    super::shutdown_current_env();
    super::shutdown_current_env();
    assert_eq!(
        releases.load(Ordering::SeqCst),
        2,
        "shutdown must release a still-live native buffer exactly once"
    );
}

#[test]
fn object_finalizer_reference_is_weak_and_can_cancel_after_collection() {
    unsafe extern "C" fn release(_env: NapiEnv, _data: *mut c_void, hint: *mut c_void) {
        (*(hint as *const AtomicUsize)).fetch_add(1, Ordering::SeqCst);
    }
    let env = test_env();
    let releases = AtomicUsize::new(0);
    for cancel in [true, false] {
        let mut scope = std::ptr::null_mut();
        let mut owner = std::ptr::null_mut();
        let mut reference = std::ptr::null_mut();
        assert_eq!(
            unsafe { napi_open_handle_scope(env, &mut scope) },
            NapiStatus::Ok
        );
        assert_eq!(
            unsafe { napi_create_object(env, &mut owner) },
            NapiStatus::Ok
        );
        assert_eq!(
            unsafe {
                napi_add_finalizer(
                    env,
                    owner,
                    std::ptr::null_mut(),
                    Some(release),
                    (&releases as *const AtomicUsize).cast_mut().cast(),
                    &mut reference,
                )
            },
            NapiStatus::Ok
        );
        crate::gc::js_gc_collect();
        super::metadata::drain_pending_finalizers();
        assert_eq!(
            releases.load(Ordering::SeqCst),
            0,
            "live owner retains metadata"
        );
        if cancel {
            assert_eq!(
                unsafe { napi_delete_reference(env, reference) },
                NapiStatus::Ok
            );
        }
        assert_eq!(
            unsafe { napi_close_handle_scope(env, scope) },
            NapiStatus::Ok
        );
        crate::gc::js_gc_collect();
        super::metadata::drain_pending_finalizers();
        assert_eq!(
            releases.load(Ordering::SeqCst),
            usize::from(!cancel),
            "a live reference must not retain the metadata or its owner"
        );
        if !cancel {
            assert_eq!(
                unsafe { napi_delete_reference(env, reference) },
                NapiStatus::Ok
            );
        }
    }
    crate::gc::js_gc_collect();
    super::metadata::drain_pending_finalizers();
    assert_eq!(releases.load(Ordering::SeqCst), 1, "finalizer runs once");
    super::shutdown_current_env();
}

#[test]
fn external_buffer_arraybuffer_released_on_worker_retirement() {
    unsafe extern "C" fn release(_env: NapiEnv, data: *mut c_void, hint: *mut c_void) {
        drop(Box::from_raw(data as *mut [u8; 3]));
        (*(hint as *const AtomicUsize)).fetch_add(1, Ordering::SeqCst);
    }
    // Installed at program start, as perry does for an addon program.
    crate::feature_hooks::js_runtime_install_node_api_host();
    let releases = std::sync::Arc::new(AtomicUsize::new(0));
    let worker_releases = releases.clone();
    std::thread::spawn(move || {
        let agent = crate::agent::enter_worker_agent();
        let env = current_env();
        let bytes = Box::into_raw(Box::new([11u8, 22, 33]));
        let mut value = std::ptr::null_mut();
        assert_eq!(
            unsafe {
                napi_create_external_arraybuffer(
                    env,
                    bytes.cast(),
                    3,
                    Some(release),
                    std::sync::Arc::as_ptr(&worker_releases).cast_mut().cast(),
                    &mut value,
                )
            },
            NapiStatus::Ok
        );
        let owner = (value_bits(env, value).unwrap() & crate::value::POINTER_MASK) as usize;
        assert!(crate::buffer::is_foreign_backed_buffer(owner));
        assert!(crate::buffer::is_array_buffer(owner));
        let mut data = std::ptr::null_mut();
        let mut length = 0;
        assert_eq!(
            unsafe { napi_get_arraybuffer_info(env, value, &mut data, &mut length) },
            NapiStatus::Ok
        );
        assert_eq!(data, bytes.cast());
        assert_eq!(length, 3);
        assert_eq!(worker_releases.load(Ordering::SeqCst), 0);
        crate::agent::retire_agent(agent);
        assert_eq!(
            worker_releases.load(Ordering::SeqCst),
            1,
            "retiring the owning agent must finalize native bytes"
        );
        crate::agent::retire_agent(agent);
        assert_eq!(worker_releases.load(Ordering::SeqCst), 1);
    })
    .join()
    .unwrap();
    assert_eq!(releases.load(Ordering::SeqCst), 1);
}

#[test]
fn double_ingress_canonicalizes_negative_nan_payloads() {
    let env = test_env();
    for bits in [
        0xfff9_0000_0000_0001_u64,
        0xffff_0000_1234_5678,
        0xfff1_0000_0000_0001,
    ] {
        let mut value = std::ptr::null_mut();
        assert_eq!(
            unsafe { napi_create_double(env, f64::from_bits(bits), &mut value) },
            NapiStatus::Ok
        );
        crate::gc::js_gc_collect();
        let mut kind = NapiValueType::Undefined;
        assert_eq!(
            unsafe { napi_typeof(env, value, &mut kind) },
            NapiStatus::Ok
        );
        assert_eq!(kind, NapiValueType::Number);
        let mut number = 0.0;
        assert_eq!(
            unsafe { napi_get_value_double(env, value, &mut number) },
            NapiStatus::Ok
        );
        assert_eq!(number.to_bits(), f64::NAN.to_bits());
    }
}

#[test]
fn an_escaped_handle_takes_the_parent_slot_reserved_at_open() {
    let env = test_env();
    let mut outer = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_open_handle_scope(env, &mut outer) },
        NapiStatus::Ok
    );
    let mut inner = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_open_escapable_handle_scope(env, &mut inner) },
        NapiStatus::Ok
    );
    let local = int32(env, 41);
    let mut escaped = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_escape_handle(env, inner, local, &mut escaped) },
        NapiStatus::Ok
    );
    assert_eq!(
        unsafe { napi_close_escapable_handle_scope(env, inner) },
        NapiStatus::Ok
    );
    // The next outer handle takes the slot the inner local had, never the
    // escapee's: that slot was the outer scope's from the inner scope's open.
    let next = int32(env, 42);
    assert_eq!(read_int32(env, escaped), 41);
    assert_eq!(read_int32(env, next), 42);
    let mut ignored = 0;
    assert_eq!(
        unsafe { napi_get_value_int32(env, local, &mut ignored) },
        NapiStatus::InvalidArg
    );
    // A closed scope's handle is no scope any more.
    assert_eq!(
        unsafe { napi_close_escapable_handle_scope(env, inner) },
        NapiStatus::HandleScopeMismatch
    );
    assert_eq!(
        unsafe { napi_close_handle_scope(env, outer) },
        NapiStatus::Ok
    );
    // The escapee belonged to the outer scope and is released with it.
    let mut reuse = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_open_handle_scope(env, &mut reuse) },
        NapiStatus::Ok
    );
    let _first = int32(env, 1);
    let _second = int32(env, 2);
    assert_eq!(
        unsafe { napi_get_value_int32(env, escaped, &mut ignored) },
        NapiStatus::InvalidArg
    );
    assert_eq!(
        unsafe { napi_close_handle_scope(env, reuse) },
        NapiStatus::Ok
    );
}

#[test]
fn handle_scopes_reuse_the_handle_stack_and_a_collection_scans_only_live_handles() {
    let env = test_env();
    let (before, live_before) = with_env(env, |env| (env.slots.len(), env.handle_top)).unwrap();
    for round in 0..100_000 {
        let mut scope = std::ptr::null_mut();
        assert_eq!(
            unsafe { napi_open_handle_scope(env, &mut scope) },
            NapiStatus::Ok
        );
        let handles = [
            int32(env, round),
            int32(env, round + 1),
            int32(env, round + 2),
        ];
        assert_eq!(read_int32(env, handles[2]), round + 2);
        assert_eq!(
            unsafe { napi_close_handle_scope(env, scope) },
            NapiStatus::Ok
        );
    }
    let (after, live_after) = with_env(env, |env| (env.slots.len(), env.handle_top)).unwrap();
    assert_eq!(live_after, live_before);
    assert!(
        after <= before.max(live_before + 3),
        "{after} slots after {before}"
    );
    crate::gc::js_gc_collect();
    assert_eq!(
        with_env(env, |env| (env.slots.len(), env.handle_top)).unwrap(),
        (live_before, live_before)
    );
}

/// node's napi_reference_unref only marks its persistent weak: a count that
/// drops to 0 and back allocates nothing. The weak target is still cleared by
/// a collection (napi_get_reference_value answers NULL), and a weak target
/// that something else keeps alive survives with its identity.
#[test]
fn reference_count_transitions_allocate_nothing_and_weak_targets_clear() {
    let env = test_env();
    let mut scope = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_open_handle_scope(env, &mut scope) },
        NapiStatus::Ok
    );
    let mut dying = std::ptr::null_mut();
    let mut kept = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_object(env, &mut dying) },
        NapiStatus::Ok
    );
    assert_eq!(
        unsafe { napi_create_object(env, &mut kept) },
        NapiStatus::Ok
    );
    let mut weak = std::ptr::null_mut();
    let mut weak_kept = std::ptr::null_mut();
    let mut strong_kept = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_reference(env, dying, 1, &mut weak) },
        NapiStatus::Ok
    );
    assert_eq!(
        unsafe { napi_create_reference(env, kept, 0, &mut weak_kept) },
        NapiStatus::Ok
    );
    assert_eq!(
        unsafe { napi_create_reference(env, kept, 1, &mut strong_kept) },
        NapiStatus::Ok
    );
    let mut count = u32::MAX;
    let before = crate::arena::arena_in_use_bytes();
    for _ in 0..100_000 {
        assert_eq!(
            unsafe { napi_reference_unref(env, weak, &mut count) },
            NapiStatus::Ok
        );
        assert_eq!(count, 0);
        assert_eq!(
            unsafe { napi_reference_ref(env, weak, &mut count) },
            NapiStatus::Ok
        );
        assert_eq!(count, 1);
    }
    assert_eq!(
        crate::arena::arena_in_use_bytes(),
        before,
        "100k unref/ref cycles must not allocate"
    );
    assert_eq!(
        unsafe { napi_reference_unref(env, weak, &mut count) },
        NapiStatus::Ok
    );
    assert_eq!(count, 0);
    let mut read = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_get_reference_value(env, weak, &mut read) },
        NapiStatus::Ok
    );
    assert_eq!(
        value_bits(env, read).unwrap(),
        value_bits(env, dying).unwrap(),
        "an uncollected weak reference reads its target"
    );
    assert_eq!(
        unsafe { napi_close_handle_scope(env, scope) },
        NapiStatus::Ok
    );
    crate::gc::js_gc_collect();
    crate::gc::js_gc_collect();

    let mut scope = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_open_handle_scope(env, &mut scope) },
        NapiStatus::Ok
    );
    let mut read = 1usize as NapiValue;
    assert_eq!(
        unsafe { napi_get_reference_value(env, weak, &mut read) },
        NapiStatus::Ok
    );
    assert!(read.is_null(), "a collected weak target reads as NULL");
    assert_eq!(
        unsafe { napi_reference_ref(env, weak, &mut count) },
        NapiStatus::Ok
    );
    assert_eq!(count, 0, "a collected weak reference cannot be revived");

    let mut via_weak = std::ptr::null_mut();
    let mut via_strong = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_get_reference_value(env, weak_kept, &mut via_weak) },
        NapiStatus::Ok
    );
    assert_eq!(
        unsafe { napi_get_reference_value(env, strong_kept, &mut via_strong) },
        NapiStatus::Ok
    );
    assert!(
        !via_weak.is_null(),
        "a weak target kept alive elsewhere survives"
    );
    assert_eq!(
        value_bits(env, via_weak).unwrap(),
        value_bits(env, via_strong).unwrap(),
        "the weak word follows the target if it moved"
    );
    assert_eq!(
        unsafe { napi_close_handle_scope(env, scope) },
        NapiStatus::Ok
    );
    for reference in [weak, weak_kept, strong_kept] {
        assert_eq!(
            unsafe { napi_delete_reference(env, reference) },
            NapiStatus::Ok
        );
    }
}

/// A settled deferred and a destroyed async context leave their lists, so
/// the root scan walks live records only (it walked every record ever made).
#[test]
fn settled_deferreds_and_destroyed_async_contexts_leave_the_root_lists() {
    let env = test_env();
    let lengths = || with_env(env, |env| (env.deferreds.len(), env.async_contexts.len())).unwrap();
    let mut deferreds = Vec::new();
    let mut promises = Vec::new();
    for _ in 0..64 {
        let mut promise = std::ptr::null_mut();
        let mut deferred = std::ptr::null_mut();
        assert_eq!(
            unsafe { napi_create_promise(env, &mut deferred, &mut promise) },
            NapiStatus::Ok
        );
        deferreds.push(deferred);
        promises.push(promise);
    }
    assert_eq!(lengths().0, 64);
    // Settle out of creation order so the moved records are re-indexed.
    for (step, index) in (0..64).map(|i| (i * 37) % 64).enumerate() {
        let value = int32(env, index as i32);
        let status = if step % 2 == 0 {
            unsafe { napi_resolve_deferred(env, deferreds[index], value) }
        } else {
            unsafe { napi_reject_deferred(env, deferreds[index], value) }
        };
        assert_eq!(status, NapiStatus::Ok);
        assert_eq!(lengths().0, 63 - step);
    }
    assert_eq!(lengths().0, 0);
    let value = int32(env, 0);
    assert_eq!(
        unsafe { napi_resolve_deferred(env, deferreds[0], value) },
        NapiStatus::InvalidArg
    );

    let name_text = CString::new("perry-test-resource").unwrap();
    let mut contexts = Vec::new();
    for _ in 0..16 {
        let mut name = std::ptr::null_mut();
        assert_eq!(
            unsafe {
                napi_create_string_utf8(env, name_text.as_ptr(), NAPI_AUTO_LENGTH, &mut name)
            },
            NapiStatus::Ok
        );
        let mut context = std::ptr::null_mut();
        assert_eq!(
            unsafe { napi_async_init(env, std::ptr::null_mut(), name, &mut context) },
            NapiStatus::Ok
        );
        contexts.push(context);
    }
    assert_eq!(lengths().1, 16);
    for (step, index) in (0..16).map(|i| (i * 5) % 16).enumerate() {
        assert_eq!(
            unsafe { napi_async_destroy(env, contexts[index]) },
            NapiStatus::Ok
        );
        assert_eq!(lengths().1, 15 - step);
    }
    assert_eq!(
        unsafe { napi_async_destroy(env, contexts[0]) },
        NapiStatus::InvalidArg
    );
}
