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

    let words = [0x0123_4567_89ab_cdef, 0xfedc_ba98_7654_3210];
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
    assert_eq!(count, 2);
    let mut output = [0u64; 2];
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
                NapiTypedarrayType::Uint32Array,
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

    assert_eq!(
        unsafe { napi_add_env_cleanup_hook(env, Some(cleanup), 2usize as *mut c_void) },
        NapiStatus::Ok
    );
    assert_eq!(
        unsafe { napi_add_env_cleanup_hook(env, Some(cleanup), 3usize as *mut c_void) },
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

    shutdown_current_env();
    assert_eq!(LIFECYCLE_SEQUENCE.load(Ordering::SeqCst), 1324);
    assert_eq!(
        ASYNC_CLEANUP_STATUS.load(Ordering::SeqCst),
        NapiStatus::Ok as usize
    );
    assert_eq!(INSTANCE_FINALIZER_DATA.load(Ordering::SeqCst), 0x8523);
    assert_eq!(INSTANCE_FINALIZER_HINT.load(Ordering::SeqCst), 0x1234);

    shutdown_current_env();
    assert_eq!(LIFECYCLE_SEQUENCE.load(Ordering::SeqCst), 1324);
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
