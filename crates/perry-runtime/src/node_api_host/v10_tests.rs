//! Node-API 9/10 surface, per-module attribution (#10456) and `napi_typeof`
//! classification of Perry's callable representations (#10461).

use super::*;
use crate::closure::ClosureHeader;
use crate::value::JSValue;
use std::cell::RefCell;
use std::ffi::{c_char, c_void, CStr};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

fn test_env() -> NapiEnv {
    crate::gc::ensure_gc_initialized();
    reset_env_for_test();
    current_env()
}

fn handle(env: NapiEnv, bits: u64) -> NapiValue {
    add_handle(env, bits).expect("live Node-API environment")
}

fn type_of(env: NapiEnv, value: NapiValue) -> NapiValueType {
    let mut value_type = NapiValueType::Undefined;
    assert_eq!(
        unsafe { napi_typeof(env, value, &mut value_type) },
        NapiStatus::Ok
    );
    value_type
}

fn utf8(env: NapiEnv, text: &CStr) -> NapiValue {
    let mut value = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_string_utf8(env, text.as_ptr(), NAPI_AUTO_LENGTH, &mut value) },
        NapiStatus::Ok
    );
    value
}

fn read_utf8(env: NapiEnv, value: NapiValue) -> String {
    let mut buffer = [0 as c_char; 256];
    let mut length = 0;
    assert_eq!(
        unsafe {
            napi_get_value_string_utf8(env, value, buffer.as_mut_ptr(), buffer.len(), &mut length)
        },
        NapiStatus::Ok
    );
    String::from_utf8_lossy(unsafe {
        std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), length)
    })
    .into_owned()
}

fn named(env: NapiEnv, object: NapiValue, name: &CStr) -> NapiValue {
    let mut value = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_get_named_property(env, object, name.as_ptr(), &mut value) },
        NapiStatus::Ok
    );
    value
}

fn take_exception(env: NapiEnv) -> NapiValue {
    let mut exception = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_get_and_clear_last_exception(env, &mut exception) },
        NapiStatus::Ok
    );
    assert!(!exception.is_null(), "an exception must be pending");
    exception
}

extern "C" fn plain_closure_body(_closure: *const ClosureHeader) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

unsafe extern "C" fn empty_callback(_env: NapiEnv, _info: NapiCallbackInfo) -> NapiValue {
    std::ptr::null_mut()
}

/// A class id no compiled program in this test binary registers.
const REGISTERED_CLASS: u32 = 0x1_0461;

#[test]
fn typeof_classifies_every_value_kind_like_javascript_typeof() {
    let env = test_env();
    let undefined = handle(env, crate::value::TAG_UNDEFINED);
    let null = handle(env, crate::value::TAG_NULL);
    let boolean = handle(env, JSValue::bool(true).bits());
    let double = handle(env, JSValue::number(1.5).bits());
    let string = utf8(env, c"text");
    let mut symbol = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_symbol(env, string, &mut symbol) },
        NapiStatus::Ok
    );
    let mut bigint = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_bigint_int64(env, 7, &mut bigint) },
        NapiStatus::Ok
    );
    let mut object = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_object(env, &mut object) },
        NapiStatus::Ok
    );
    let mut array = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_array(env, &mut array) },
        NapiStatus::Ok
    );
    let mut external = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            napi_create_external(
                env,
                0x10461usize as *mut c_void,
                None,
                std::ptr::null_mut(),
                &mut external,
            )
        },
        NapiStatus::Ok
    );
    let mut native_function = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            napi_create_function(
                env,
                c"native".as_ptr(),
                NAPI_AUTO_LENGTH,
                Some(empty_callback),
                std::ptr::null_mut(),
                &mut native_function,
            )
        },
        NapiStatus::Ok
    );
    let closure = crate::closure::js_closure_alloc(plain_closure_body as *const u8, 0);
    let closure = handle(env, JSValue::pointer(closure.cast()).bits());
    let no_arguments: [f64; 0] = [];
    let bound = unsafe {
        crate::closure::js_function_bind(
            f64::from_bits(value_bits(env, closure).unwrap()),
            no_arguments.as_ptr(),
            0,
        )
    };
    let bound = handle(env, bound.to_bits());

    unsafe { crate::object::js_register_class_id(REGISTERED_CLASS) };
    // A class declaration's value: `INT32_TAG | class_id` (#10461).
    let class_ref = handle(env, JSValue::int32(REGISTERED_CLASS as i32).bits());
    // A class expression's value: an object stamped as a class.
    let class_object = crate::object::js_object_alloc(REGISTERED_CLASS, 0);
    let class_object = handle(env, JSValue::pointer(class_object.cast()).bits());
    crate::object::js_object_mark_class(
        JSValue::from_bits(value_bits(env, class_object).unwrap()).as_pointer::<u8>() as i64,
    );

    for (name, value, expected) in [
        ("undefined", undefined, NapiValueType::Undefined),
        ("null", null, NapiValueType::Null),
        ("boolean", boolean, NapiValueType::Boolean),
        ("number", double, NapiValueType::Number),
        ("string", string, NapiValueType::String),
        ("symbol", symbol, NapiValueType::Symbol),
        ("bigint", bigint, NapiValueType::Bigint),
        ("object", object, NapiValueType::Object),
        ("array", array, NapiValueType::Object),
        ("external", external, NapiValueType::External),
        ("native function", native_function, NapiValueType::Function),
        ("closure", closure, NapiValueType::Function),
        ("bound function", bound, NapiValueType::Function),
        ("class ref", class_ref, NapiValueType::Function),
        ("class object", class_object, NapiValueType::Function),
    ] {
        assert_eq!(type_of(env, value), expected, "napi_typeof({name})");
    }
}

#[test]
fn created_integers_stay_numbers_when_a_class_id_shares_their_value() {
    let env = test_env();
    unsafe { crate::object::js_register_class_id(REGISTERED_CLASS) };
    let mut int32 = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_int32(env, REGISTERED_CLASS as i32, &mut int32) },
        NapiStatus::Ok
    );
    let mut uint32 = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_uint32(env, REGISTERED_CLASS, &mut uint32) },
        NapiStatus::Ok
    );
    for value in [int32, uint32] {
        assert_eq!(type_of(env, value), NapiValueType::Number);
        let mut read = 0;
        assert_eq!(
            unsafe { napi_get_value_int32(env, value, &mut read) },
            NapiStatus::Ok
        );
        assert_eq!(read, REGISTERED_CLASS as i32);
    }

    // The constructor itself is not a number: Node reports
    // napi_number_expected for a function, and ToObject returns it unchanged.
    let class_ref = handle(env, JSValue::int32(REGISTERED_CLASS as i32).bits());
    let mut int_out = 0;
    assert_eq!(
        unsafe { napi_get_value_int32(env, class_ref, &mut int_out) },
        NapiStatus::NumberExpected
    );
    let mut double_out = 0.0;
    assert_eq!(
        unsafe { napi_get_value_double(env, class_ref, &mut double_out) },
        NapiStatus::NumberExpected
    );
    let mut as_object = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_coerce_to_object(env, class_ref, &mut as_object) },
        NapiStatus::Ok
    );
    let mut same = false;
    assert_eq!(
        unsafe { napi_strict_equals(env, as_object, class_ref, &mut same) },
        NapiStatus::Ok
    );
    assert!(same, "ToObject(class) must return the class itself");
}

#[test]
fn module_versions_follow_node_rules_up_to_version_10() {
    assert_eq!(NAPI_VERSION, 10);
    assert_eq!(effective_module_version(None), Ok(8));
    for (declared, effective) in [(1, 8), (8, 8), (9, 9), (10, 10)] {
        assert_eq!(effective_module_version(Some(declared)), Ok(effective));
    }
    let too_new = effective_module_version(Some(11)).unwrap_err();
    assert!(too_new.contains("version 11"), "{too_new}");
    assert!(too_new.contains("1 through 10"), "{too_new}");
    assert!(effective_module_version(Some(0)).is_err());
    let experimental = effective_module_version(Some(NAPI_VERSION_EXPERIMENTAL)).unwrap_err();
    assert!(experimental.contains("NAPI_EXPERIMENTAL"), "{experimental}");
}

#[test]
fn symbol_for_and_syntax_errors_match_node_api_9() {
    let env = test_env();
    let mut first = std::ptr::null_mut();
    let mut second = std::ptr::null_mut();
    assert_eq!(
        unsafe { node_api_symbol_for(env, c"perry.key".as_ptr(), NAPI_AUTO_LENGTH, &mut first) },
        NapiStatus::Ok
    );
    // An explicit length selects the same registry key.
    assert_eq!(
        unsafe { node_api_symbol_for(env, c"perry.key!".as_ptr(), 9, &mut second) },
        NapiStatus::Ok
    );
    assert_eq!(type_of(env, first), NapiValueType::Symbol);
    let mut same = false;
    assert_eq!(
        unsafe { napi_strict_equals(env, first, second, &mut same) },
        NapiStatus::Ok
    );
    assert!(same, "Symbol.for must return the registered symbol");
    let mut unique = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_symbol(env, utf8(env, c"perry.key"), &mut unique) },
        NapiStatus::Ok
    );
    assert_eq!(
        unsafe { napi_strict_equals(env, first, unique, &mut same) },
        NapiStatus::Ok
    );
    assert!(!same);

    let mut error = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            node_api_create_syntax_error(
                env,
                utf8(env, c"ERR_SYNTAX"),
                utf8(env, c"bad token"),
                &mut error,
            )
        },
        NapiStatus::Ok
    );
    assert_eq!(read_utf8(env, named(env, error, c"name")), "SyntaxError");
    assert_eq!(read_utf8(env, named(env, error, c"message")), "bad token");
    assert_eq!(read_utf8(env, named(env, error, c"code")), "ERR_SYNTAX");

    assert_eq!(
        unsafe { node_api_throw_syntax_error(env, c"ERR_THROWN".as_ptr(), c"thrown".as_ptr()) },
        NapiStatus::Ok
    );
    let thrown = take_exception(env);
    assert_eq!(read_utf8(env, named(env, thrown, c"name")), "SyntaxError");
    assert_eq!(read_utf8(env, named(env, thrown, c"code")), "ERR_THROWN");
}

static EXTERNAL_FINALIZED: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn external_string_finalizer(
    _env: NapiEnv,
    data: *mut c_void,
    hint: *mut c_void,
) {
    assert_eq!(hint as usize, 0x10456);
    // Clobber the addon's storage the way `free` may: the string must already
    // own a copy.
    *data.cast::<u8>() = b'#';
    EXTERNAL_FINALIZED.fetch_add(1, Ordering::SeqCst);
}

#[test]
fn property_keys_and_external_strings_match_node_api_10() {
    let env = test_env();
    let mut object = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_object(env, &mut object) },
        NapiStatus::Ok
    );
    let mut latin1_key = std::ptr::null_mut();
    let mut utf8_key = std::ptr::null_mut();
    let mut utf16_key = std::ptr::null_mut();
    let utf16 = [u16::from(b'k'), 0x00e9];
    unsafe {
        assert_eq!(
            node_api_create_property_key_latin1(env, c"k1".as_ptr(), 2, &mut latin1_key),
            NapiStatus::Ok
        );
        assert_eq!(
            node_api_create_property_key_utf8(env, c"k2".as_ptr(), NAPI_AUTO_LENGTH, &mut utf8_key),
            NapiStatus::Ok
        );
        assert_eq!(
            node_api_create_property_key_utf16(env, utf16.as_ptr(), utf16.len(), &mut utf16_key),
            NapiStatus::Ok
        );
        for (key, text) in [
            (latin1_key, c"one"),
            (utf8_key, c"two"),
            (utf16_key, c"three"),
        ] {
            assert_eq!(
                napi_set_property(env, object, key, utf8(env, text)),
                NapiStatus::Ok
            );
        }
    }
    assert_eq!(read_utf8(env, named(env, object, c"k1")), "one");
    assert_eq!(read_utf8(env, named(env, object, c"k2")), "two");
    assert_eq!(read_utf8(env, named(env, object, c"k\u{e9}")), "three");

    EXTERNAL_FINALIZED.store(0, Ordering::SeqCst);
    let mut latin1 = *b"external\xe9";
    let mut value = std::ptr::null_mut();
    let mut copied = false;
    assert_eq!(
        unsafe {
            node_api_create_external_string_latin1(
                env,
                latin1.as_mut_ptr().cast(),
                latin1.len(),
                Some(external_string_finalizer),
                0x10456usize as *mut c_void,
                &mut value,
                &mut copied,
            )
        },
        NapiStatus::Ok
    );
    assert!(copied, "a copied external string must report copied = true");
    assert_eq!(
        EXTERNAL_FINALIZED.load(Ordering::SeqCst),
        1,
        "a copied external string's finalizer runs before the call returns"
    );
    assert_eq!(read_utf8(env, value), "external\u{e9}");

    let mut utf16 = [u16::from(b'u'), 0xd83d, 0xde00];
    assert_eq!(
        unsafe {
            node_api_create_external_string_utf16(
                env,
                utf16.as_mut_ptr(),
                utf16.len(),
                Some(external_string_finalizer),
                0x10456usize as *mut c_void,
                &mut value,
                std::ptr::null_mut(),
            )
        },
        NapiStatus::Ok
    );
    assert_eq!(EXTERNAL_FINALIZED.load(Ordering::SeqCst), 2);
    assert_eq!(read_utf8(env, value), "u\u{1f600}");

    // A failed creation leaves ownership with the addon: no finalizer.
    assert_eq!(
        unsafe {
            node_api_create_external_string_latin1(
                env,
                latin1.as_mut_ptr().cast(),
                latin1.len(),
                Some(external_string_finalizer),
                0x10456usize as *mut c_void,
                std::ptr::null_mut(),
                &mut copied,
            )
        },
        NapiStatus::InvalidArg
    );
    assert_eq!(EXTERNAL_FINALIZED.load(Ordering::SeqCst), 2);
}

#[test]
fn buffer_from_arraybuffer_shares_storage_and_checks_its_window() {
    let env = test_env();
    let mut arraybuffer = std::ptr::null_mut();
    let mut bytes = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_arraybuffer(env, 8, &mut bytes, &mut arraybuffer) },
        NapiStatus::Ok
    );
    let mut buffer = std::ptr::null_mut();
    assert_eq!(
        unsafe { node_api_create_buffer_from_arraybuffer(env, arraybuffer, 2, 4, &mut buffer) },
        NapiStatus::Ok
    );
    let mut is_buffer = false;
    assert_eq!(
        unsafe { napi_is_buffer(env, buffer, &mut is_buffer) },
        NapiStatus::Ok
    );
    assert!(is_buffer);
    let mut data = std::ptr::null_mut();
    let mut length = 0;
    assert_eq!(
        unsafe { napi_get_buffer_info(env, buffer, &mut data, &mut length) },
        NapiStatus::Ok
    );
    assert_eq!(length, 4);
    assert_eq!(data, unsafe { bytes.cast::<u8>().add(2) }.cast());
    unsafe { *data.cast::<u8>() = 0x5a };
    assert_eq!(unsafe { *bytes.cast::<u8>().add(2) }, 0x5a);

    // Node throws ERR_OUT_OF_RANGE and returns the throw's own status.
    let mut ignored = std::ptr::null_mut();
    assert_eq!(
        unsafe { node_api_create_buffer_from_arraybuffer(env, arraybuffer, 6, 4, &mut ignored) },
        NapiStatus::Ok
    );
    let range_error = take_exception(env);
    assert_eq!(
        read_utf8(env, named(env, range_error, c"name")),
        "RangeError"
    );
    assert_eq!(
        read_utf8(env, named(env, range_error, c"code")),
        "ERR_OUT_OF_RANGE"
    );
    assert_eq!(
        unsafe {
            node_api_create_buffer_from_arraybuffer(env, arraybuffer, usize::MAX, 2, &mut ignored)
        },
        NapiStatus::Ok
    );
    take_exception(env);
    assert_eq!(
        unsafe { node_api_create_buffer_from_arraybuffer(env, buffer, 0, 1, &mut ignored) },
        NapiStatus::InvalidArg
    );
}

thread_local! {
    static SEEN_MODULE_FILE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn module_file_name(env: NapiEnv) -> String {
    let mut name = std::ptr::null();
    assert_eq!(
        unsafe { node_api_get_module_file_name(env, &mut name) },
        NapiStatus::Ok
    );
    unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned()
}

unsafe extern "C" fn record_module_callback(env: NapiEnv, _info: NapiCallbackInfo) -> NapiValue {
    SEEN_MODULE_FILE.with(|seen| seen.borrow_mut().push(module_file_name(env)));
    std::ptr::null_mut()
}

#[test]
fn module_file_name_follows_the_addon_whose_code_runs() {
    let env = test_env();
    assert_eq!(module_file_name(env), "", "no addon has loaded yet");
    let first = register_module(env, Path::new("/opt/perry app/first.node"), 10);
    let second = register_module(env, Path::new("/opt/perry app/second.node"), 9);
    assert_eq!((first, second), (Some(0), Some(1)));
    const FIRST: &str = "file:///opt/perry%20app/first.node";
    const SECOND: &str = "file:///opt/perry%20app/second.node";

    let function = with_active_module(env, first, || {
        assert_eq!(module_file_name(env), FIRST);
        with_active_module(env, second, || assert_eq!(module_file_name(env), SECOND));
        assert_eq!(module_file_name(env), FIRST);
        let mut function = std::ptr::null_mut();
        assert_eq!(
            unsafe {
                napi_create_function(
                    env,
                    c"record".as_ptr(),
                    NAPI_AUTO_LENGTH,
                    Some(record_module_callback),
                    std::ptr::null_mut(),
                    &mut function,
                )
            },
            NapiStatus::Ok
        );
        function
    });
    assert_eq!(active_module(env), None);

    // Called while the second addon is running, the first addon's function
    // still observes its own module.
    SEEN_MODULE_FILE.with(|seen| seen.borrow_mut().clear());
    with_active_module(env, second, || {
        let mut receiver = std::ptr::null_mut();
        unsafe {
            assert_eq!(napi_get_undefined(env, &mut receiver), NapiStatus::Ok);
            assert_eq!(
                napi_call_function(
                    env,
                    receiver,
                    function,
                    0,
                    std::ptr::null(),
                    std::ptr::null_mut()
                ),
                NapiStatus::Ok
            );
        }
        assert_eq!(module_file_name(env), SECOND);
    });
    SEEN_MODULE_FILE.with(|seen| assert_eq!(*seen.borrow(), [FIRST.to_string()]));
}

thread_local! {
    static UNCAUGHT_CALLBACK_ERRORS: RefCell<usize> = const { RefCell::new(0) };
}

extern "C" fn count_uncaught_callback_error(_closure: *const ClosureHeader, _error: f64) -> f64 {
    UNCAUGHT_CALLBACK_ERRORS.with(|count| *count.borrow_mut() += 1);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn listen_for_uncaught_exceptions() {
    crate::os::test_clear_process_event_listeners();
    UNCAUGHT_CALLBACK_ERRORS.with(|count| *count.borrow_mut() = 0);
    crate::closure::js_register_closure_arity(count_uncaught_callback_error as *const u8, 1);
    let listener = crate::closure::js_closure_alloc(count_uncaught_callback_error as *const u8, 0);
    let listener = JSValue::pointer(listener.cast()).bits();
    let event = crate::string::js_string_from_bytes(b"uncaughtException".as_ptr(), 17);
    let event = JSValue::string_ptr(event).bits();
    let _ = crate::os::js_process_on(event as i64, listener as i64);
}

fn uncaught_callback_errors() -> usize {
    UNCAUGHT_CALLBACK_ERRORS.with(|count| *count.borrow())
}

static THROWING_TSFN_CALLS: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn throwing_tsfn_call_js(
    env: NapiEnv,
    _function: NapiValue,
    _context: *mut c_void,
    _data: *mut c_void,
) {
    THROWING_TSFN_CALLS.fetch_add(1, Ordering::SeqCst);
    assert_eq!(
        napi_throw_error(env, std::ptr::null(), c"tsfn failed".as_ptr()),
        NapiStatus::Ok
    );
}

fn call_throwing_tsfn(env: NapiEnv, module: Option<u32>) {
    let name = utf8(env, c"throwing");
    let mut tsfn = std::ptr::null_mut();
    with_active_module(env, module, || {
        assert_eq!(
            unsafe {
                napi_create_threadsafe_function(
                    env,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    name,
                    0,
                    1,
                    std::ptr::null_mut(),
                    None,
                    std::ptr::null_mut(),
                    Some(throwing_tsfn_call_js),
                    &mut tsfn,
                )
            },
            NapiStatus::Ok
        );
    });
    unsafe {
        assert_eq!(
            napi_call_threadsafe_function(
                tsfn,
                std::ptr::null_mut(),
                NapiThreadsafeFunctionCallMode::Nonblocking
            ),
            NapiStatus::Ok
        );
        assert_eq!(
            napi_release_threadsafe_function(tsfn, NapiThreadsafeFunctionReleaseMode::Release),
            NapiStatus::Ok
        );
    }
    process_pending();
}

#[test]
fn callback_exceptions_follow_the_declaring_modules_policy() {
    let env = test_env();
    listen_for_uncaught_exceptions();
    let legacy = register_module(env, Path::new("/opt/legacy.node"), 8);
    let current = register_module(env, Path::new("/opt/current.node"), 10);

    // Node-API 10: an exception left by a TSFN callback is uncaught.
    THROWING_TSFN_CALLS.store(0, Ordering::SeqCst);
    call_throwing_tsfn(env, current);
    assert_eq!(THROWING_TSFN_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(uncaught_callback_errors(), 1);
    assert_eq!(pending_exception(env), None);

    // Below 10, Node emits DEP0168 instead and drops the exception, which
    // must not leak into the next Node-API call either.
    call_throwing_tsfn(env, legacy);
    assert_eq!(THROWING_TSFN_CALLS.load(Ordering::SeqCst), 2);
    assert_eq!(uncaught_callback_errors(), 1);
    assert_eq!(pending_exception(env), None);

    // Finalizers and async-work completion enforce the policy for every
    // version; a shutting-down environment drops the exception.
    assert_eq!(
        unsafe { napi_throw_error(env, std::ptr::null(), c"finalizer".as_ptr()) },
        NapiStatus::Ok
    );
    settle_callback_exception(env, legacy, true);
    assert_eq!(uncaught_callback_errors(), 2);
    with_env_mut(env, |env| env.shutting_down = true);
    assert_eq!(
        unsafe { napi_throw_error(env, std::ptr::null(), c"late".as_ptr()) },
        NapiStatus::Ok
    );
    settle_callback_exception(env, current, true);
    assert_eq!(uncaught_callback_errors(), 2);
    assert_eq!(pending_exception(env), None);
    crate::os::test_clear_process_event_listeners();
}

static ASYNC_THROWN: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn quiet_execute(_env: NapiEnv, _data: *mut c_void) {}

unsafe extern "C" fn throwing_complete(env: NapiEnv, _status: NapiStatus, _data: *mut c_void) {
    ASYNC_THROWN.fetch_add(1, Ordering::SeqCst);
    assert_eq!(
        napi_throw_error(env, std::ptr::null(), c"complete failed".as_ptr()),
        NapiStatus::Ok
    );
}

#[test]
fn async_work_completion_exceptions_are_uncaught() {
    ASYNC_THROWN.store(0, Ordering::SeqCst);
    let env = test_env();
    listen_for_uncaught_exceptions();
    let legacy = register_module(env, Path::new("/opt/legacy.node"), 8);
    let name = utf8(env, c"work");
    let mut work = std::ptr::null_mut();
    with_active_module(env, legacy, || {
        assert_eq!(
            unsafe {
                napi_create_async_work(
                    env,
                    std::ptr::null_mut(),
                    name,
                    Some(quiet_execute),
                    Some(throwing_complete),
                    std::ptr::null_mut(),
                    &mut work,
                )
            },
            NapiStatus::Ok
        );
    });
    assert_eq!(unsafe { napi_queue_async_work(env, work) }, NapiStatus::Ok);
    for _ in 0..500 {
        process_pending();
        if ASYNC_THROWN.load(Ordering::SeqCst) != 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(ASYNC_THROWN.load(Ordering::SeqCst), 1);
    assert_eq!(uncaught_callback_errors(), 1);
    assert_eq!(pending_exception(env), None);
    crate::os::test_clear_process_event_listeners();
}
