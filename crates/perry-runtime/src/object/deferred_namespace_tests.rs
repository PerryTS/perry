use super::*;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

static TARGET: AtomicU64 = AtomicU64::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);

extern "C" fn evaluate() -> f64 {
    CALLS.fetch_add(1, Ordering::SeqCst);
    f64::from_bits(TARGET.load(Ordering::SeqCst))
}

fn fixture() -> f64 {
    let keys = [b"value".as_ptr(), b"then".as_ptr()];
    let lengths = [5, 4];
    let values = [42.0, 7.0];
    let live = [0, 0];
    TARGET.store(
        js_create_namespace(
            2,
            keys.as_ptr(),
            lengths.as_ptr(),
            values.as_ptr(),
            live.as_ptr(),
        )
        .to_bits(),
        Ordering::SeqCst,
    );
    CALLS.store(0, Ordering::SeqCst);
    deferred_namespace::js_create_deferred_namespace(evaluate as *const () as i64)
}

#[test]
fn deferred_namespace_get_delays_evaluation_until_a_string_property() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let ns = fixture();
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
    assert_eq!(
        js_object_get_prototype_of(ns).to_bits(),
        crate::value::TAG_NULL
    );
    assert_eq!(
        js_object_is_extensible(ns).to_bits(),
        crate::value::TAG_FALSE
    );
    let then = crate::string::js_string_from_bytes(b"then".as_ptr(), 4);
    let ptr = JSValue::from_bits(ns.to_bits()).as_pointer::<ObjectHeader>();
    assert_eq!(
        js_object_get_field_by_name(ptr, then).bits(),
        crate::value::TAG_UNDEFINED
    );
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
    let key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    assert_eq!(js_object_get_field_by_name(ptr, key).as_number(), 42.0);
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
}

fn key(name: &[u8]) -> f64 {
    crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
        name.as_ptr(),
        name.len() as u32,
    ) as i64)
}

#[test]
fn deferred_namespace_reflection_and_missing_properties_trigger_evaluation() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let operations: &[fn(f64)] = &[
        |ns| {
            js_object_has_property(ns, key(b"missing"));
        },
        |ns| {
            js_object_has_own(ns, key(b"missing"));
        },
        |ns| {
            js_object_get_own_property_descriptor(ns, key(b"missing"));
        },
        |ns| {
            let ptr =
                JSValue::from_bits(ns.to_bits()).as_pointer::<ObjectHeader>() as *mut ObjectHeader;
            js_object_delete_field(
                ptr,
                crate::value::js_get_string_pointer_unified(key(b"missing"))
                    as *const crate::StringHeader,
            );
        },
        |ns| {
            let desc = crate::value::js_nanbox_pointer(js_object_alloc(0, 0) as i64);
            assert_eq!(
                crate::proxy::js_reflect_define_property(ns, key(b"missing"), desc).to_bits(),
                crate::value::TAG_FALSE
            );
        },
        |ns| {
            js_object_get_own_property_names(ns);
        },
        |ns| {
            js_object_keys_value(ns);
        },
        |ns| unsafe {
            crate::symbol::js_object_get_own_property_symbols(ns);
        },
    ];
    for operation in operations {
        let ns = fixture();
        operation(ns);
        assert!(CALLS.load(Ordering::SeqCst) > 0);
    }
}

#[test]
fn deferred_namespace_symbols_then_and_assignment_do_not_evaluate() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let ns = fixture();
    let tag_symbol =
        crate::value::js_nanbox_pointer(crate::symbol::well_known_symbol("toStringTag") as i64);
    let tag = unsafe { crate::symbol::js_object_get_symbol_property(ns, tag_symbol) };
    let ptr = crate::value::js_get_string_pointer_unified(tag) as *const crate::StringHeader;
    assert_eq!(
        unsafe {
            std::slice::from_raw_parts(crate::string::string_data(ptr), (*ptr).byte_len as usize)
        },
        b"Deferred Module"
    );
    assert_eq!(
        js_object_has_property(ns, key(b"then")).to_bits(),
        crate::value::TAG_FALSE
    );
    assert_eq!(
        js_object_get_own_property_descriptor(ns, key(b"then")).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    assert_eq!(
        crate::proxy::js_reflect_set(ns, key(b"value"), 10.0, ns).to_bits(),
        crate::value::TAG_FALSE,
    );
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
    let symbols = unsafe { crate::symbol::js_object_get_own_property_symbols(ns) }
        as *const crate::ArrayHeader;
    assert_eq!(crate::array::js_array_length(symbols), 1);
    assert!(CALLS.load(Ordering::SeqCst) > 0);
}

#[test]
fn deferred_namespace_inherited_access_and_distinct_receiver_set() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let ns = fixture();
    let child = js_object_create(ns);
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
    assert!(!property_ops::ordinary_set_with_receiver(
        ns,
        key(b"value"),
        99.0,
        child
    ));
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
    assert_eq!(
        js_object_has_property(child, key(b"value")).to_bits(),
        crate::value::TAG_TRUE
    );
    let ptr = JSValue::from_bits(child.to_bits()).as_pointer::<ObjectHeader>();
    let name =
        crate::value::js_get_string_pointer_unified(key(b"value")) as *const crate::StringHeader;
    assert_eq!(js_object_get_field_by_name(ptr, name).as_number(), 42.0);
    assert!(CALLS.load(Ordering::SeqCst) > 0);
    let names = js_object_get_own_property_names(ns);
    let names = JSValue::from_bits(names.to_bits()).as_pointer::<crate::ArrayHeader>();
    assert_eq!(crate::array::js_array_length(names), 2);
    let keys = js_object_keys_value(ns);
    assert_eq!(crate::array::js_array_length(keys), 1);
    let name = crate::array::js_array_get(keys, 0);
    let mut short = [0; crate::value::SHORT_STRING_MAX_LEN];
    assert_eq!(
        unsafe { crate::string::js_string_key_bytes(name, &mut short) },
        Some(b"value".as_slice())
    );
}

static READY_A: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static READY_B: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

extern "C" fn ready_a_dependencies() -> i32 {
    unsafe {
        deferred_namespace::js_deferred_module_ready(
            READY_B.as_ptr() as i64,
            ready_b_dependencies as *const () as i64,
            0,
        )
    }
}
extern "C" fn ready_b_dependencies() -> i32 {
    unsafe {
        deferred_namespace::js_deferred_module_ready(
            READY_A.as_ptr() as i64,
            ready_a_dependencies as *const () as i64,
            0,
        )
    }
}

#[test]
fn deferred_namespace_readiness_prunes_evaluated_cycles_and_clears_visited() {
    let ready = |has_tla| unsafe {
        deferred_namespace::js_deferred_module_ready(
            READY_A.as_ptr() as i64,
            ready_a_dependencies as *const () as i64,
            has_tla,
        )
    };
    READY_A.store(0, Ordering::SeqCst);
    READY_B.store(0, Ordering::SeqCst);
    assert_eq!(ready(0), 1, "linked cycle is ready");
    READY_B.store(1, Ordering::SeqCst);
    assert_eq!(ready(0), 0, "evaluating dependency blocks readiness");
    assert_eq!(ready(0), 0, "visited state never leaks into the next check");
    READY_A.store(2, Ordering::SeqCst);
    assert_eq!(ready(0), 1, "evaluated module prunes its dependencies");
    READY_A.store(0, Ordering::SeqCst);
    READY_B.store(0, Ordering::SeqCst);
    assert_eq!(ready(0), 1);
    assert_eq!(
        ready(1),
        0,
        "unexecuted TLA cannot be evaluated synchronously"
    );
    READY_A.store(2, Ordering::SeqCst);
    assert_eq!(ready(1), 1);
}

#[test]
fn deferred_namespace_method_lookup_reads_before_call_arguments() {
    let ns = fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let ns = scope.root_nanbox_f64(ns);
    let key = unsafe { crate::string::js_string_from_bytes(b"value".as_ptr(), 5) };
    let id = crate::value::js_nanbox_string(key as i64).to_bits() as i64;
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
    // A non-callable value still has to be read now, not deferred to the
    // post-argument by-name dispatcher. The call half handles TypeError later.
    let value = unsafe { super::method_call::js_method_prepare(ns.get_nanbox_f64(), id) };
    assert_eq!(value, 42.0);
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
}

#[test]
fn deferred_namespace_zero_argument_dispatch_performs_property_get() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let ns = fixture();
    // Even a non-callable export must be read (evaluating the module) before
    // the call raises TypeError. A by-name lookup of the empty wrapper skips it.
    let result = crate::exception::js_call_catching(|| unsafe {
        super::native_call_method::js_native_call_method(
            ns,
            b"value".as_ptr() as *const i8,
            5,
            std::ptr::null(),
            0,
        )
    });
    assert!(result.is_err());
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
}
