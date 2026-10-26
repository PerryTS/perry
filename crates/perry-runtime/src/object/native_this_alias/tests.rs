use super::*;
use crate::codegen_abi::LayoutWord;
use crate::object::shapes::store_kind::receiver_admits_plain_store;
use crate::object::shapes::{object_shape_stamp, shape_object_kind_for_word, ShapeObjectKind};
use std::sync::atomic::{AtomicU64, Ordering};

// Test-only id in the native handle band; the extension declines every other id.
const HANDLE: i64 = 0xe1725;
per_test_global! {
    static NATIVE_VALUE: AtomicU64 = AtomicU64::new(0);
}

unsafe extern "C" fn property(handle: i64, name: *const u8, len: usize, out: *mut f64) -> i32 {
    if handle != HANDLE || std::slice::from_raw_parts(name, len) != b"socket" {
        return 0;
    }
    *out = f64::from_bits(NATIVE_VALUE.load(Ordering::Relaxed));
    1
}

/// The front probe first, then the complete slow read when the front has no
/// answer: the read a compiled `obj.key` performs.
fn read_key(obj: &crate::gc::RuntimeHandle<'_>, key: &crate::gc::RuntimeHandle<'_>) -> f64 {
    obj.with_const_ptr(|o: *const super::super::ObjectHeader| {
        key.with_const_ptr(|k| {
            let front = unsafe {
                match crate::string::existing_property_atom(k) {
                    Some(atom) => crate::object::layout_probe::perry_probe_get(
                        super::super::shapes::ordinary_dir_addr(),
                        (o as i64).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR as i64),
                        atom as u64 | crate::value::STRING_TAG,
                        crate::string::property_atom_hash(atom),
                    ),
                    None => f64::from_bits(crate::value::TAG_HOLE),
                }
            };
            if front.to_bits() != crate::value::TAG_HOLE {
                return front;
            }
            crate::object::js_object_get_field_slow(o as i64, k)
        })
    })
}

fn kind_of(obj: &crate::gc::RuntimeHandle<'_>) -> (LayoutWord, Option<ShapeObjectKind>, bool) {
    obj.with_const_ptr(|o: *const super::super::ObjectHeader| unsafe {
        let id = object_shape_stamp(o);
        (
            id,
            shape_object_kind_for_word(id),
            receiver_admits_plain_store(o),
        )
    })
}

#[test]
fn inline_cache_reads_native_alias_state_without_caching_a_miss() {
    unsafe {
        super::super::class_handles::js_register_handle_property_dispatch_extension(property)
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(super::super::js_object_alloc(0, 0));
    let sibling = scope.root_raw_mut_ptr(super::super::js_object_alloc(0, 0));
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(b"socket".as_ptr(), 6));
    let read = |obj: &crate::gc::RuntimeHandle<'_>| read_key(obj, &key);
    // Prime a miss before the object becomes native-backed.
    assert_eq!(read(&object).to_bits(), crate::value::TAG_UNDEFINED);
    let (birth_id, birth_kind, birth_admitted) = kind_of(&object);
    assert_eq!(birth_kind, Some(ShapeObjectKind::OrdinaryUnmarked));
    assert!(
        !birth_admitted,
        "a runtime-born record does not admit plain stores"
    );
    object.with_const_ptr(|o: *const super::super::ObjectHeader| {
        register_this_to_handle_alias(
            crate::value::js_nanbox_pointer(o as i64),
            crate::value::js_nanbox_pointer(HANDLE),
            true,
        )
    });
    // The premises of the loop below, each its own assertion so a failure names
    // the broken link (alias recorded; dispatcher present; dispatcher claims
    // the handle) rather than only "the read came back undefined".
    object.with_const_ptr(|o: *const super::super::ObjectHeader| {
        assert!(
            unsafe { object_alias(o) }.is_some(),
            "the alias must be recorded on the object"
        );
    });
    // The alias is a shape fact: a distinct shape of the unmarked alias kind,
    // and registration grants no plain-store permission.
    let (alias_id, alias_kind, alias_admitted) = kind_of(&object);
    assert_ne!(alias_id, birth_id, "alias installation must move the shape");
    assert_eq!(
        alias_kind,
        Some(ShapeObjectKind::OrdinaryUnmarkedNativeAlias)
    );
    assert!(!alias_admitted, "the alias must not admit plain stores");
    assert!(
        super::super::class_handles::handle_property_dispatch().is_some(),
        "the extension must make a property dispatcher available"
    );
    NATIVE_VALUE.store(41.0_f64.to_bits(), Ordering::Relaxed);
    let forwarded = object.with_const_ptr(|o: *const super::super::ObjectHeader| {
        alias_forward_property_read(crate::value::js_nanbox_pointer(o as i64), "socket")
    });
    assert_eq!(
        forwarded.map(f64::to_bits),
        Some(41.0_f64.to_bits()),
        "the alias must forward `socket` to the registered native dispatcher"
    );
    for value in [41.0_f64, 42.0] {
        NATIVE_VALUE.store(value.to_bits(), Ordering::Relaxed);
        let got = read(&object);
        assert_eq!(
            got.to_bits(),
            value.to_bits(),
            "the alias must forward the read to the native state (got {:#x})",
            got.to_bits()
        );
        assert_eq!(read(&sibling).to_bits(), crate::value::TAG_UNDEFINED);
    }
    // An ordinary own property must still take precedence over native state.
    object.with_mut_ptr(|o| {
        key.with_const_ptr(|k| {
            super::super::js_object_set_field_by_name(o, k, 99.0);
        })
    });
    assert_eq!(read(&object), 99.0);
    let (_, own_kind, own_admitted) = kind_of(&object);
    assert_eq!(own_kind, Some(ShapeObjectKind::OrdinaryUnmarkedNativeAlias));
    assert!(
        !own_admitted,
        "an own key must not grant plain-store permission"
    );
}

/// Control: a receiver that already admits plain stores keeps that permission
/// and takes the admitted alias kind.
#[test]
fn marked_receiver_takes_the_admitted_native_alias_kind() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(super::super::js_object_alloc_plain(0));
    let (_, kind, admitted) = kind_of(&object);
    assert_eq!(kind, Some(ShapeObjectKind::Ordinary));
    assert!(admitted);
    object.with_const_ptr(|o: *const super::super::ObjectHeader| {
        register_this_to_handle_alias(
            crate::value::js_nanbox_pointer(o as i64),
            crate::value::js_nanbox_pointer(HANDLE),
            true,
        )
    });
    let (_, kind, admitted) = kind_of(&object);
    assert_eq!(kind, Some(ShapeObjectKind::OrdinaryNativeAlias));
    assert!(admitted, "the alias must not remove plain-store permission");
}

thread_local! {
    static HTTP_ARMED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn armed() -> bool {
    HTTP_ARMED.with(std::cell::Cell::get)
}

/// Mock of perry-ext-http's export dispatcher: `http.Server` constructs the
/// test handle. Answers only on the thread that armed it.
unsafe extern "C" fn http_dispatch(
    module: *const u8,
    module_len: usize,
    method: *const u8,
    method_len: usize,
    _args: *const f64,
    _args_len: usize,
) -> f64 {
    if armed()
        && std::slice::from_raw_parts(module, module_len) == b"http"
        && std::slice::from_raw_parts(method, method_len) == b"Server"
    {
        return crate::value::js_nanbox_pointer(HANDLE);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Mock of the stdlib's primary handle property dispatcher: a server's
/// `timeout` is 0, as in node.
unsafe extern "C" fn server_property(handle: i64, name: *const u8, len: usize) -> f64 {
    if armed() && handle == HANDLE && std::slice::from_raw_parts(name, len) == b"timeout" {
        return 0.0;
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// The production route of the TypeScript repro: `const r = Object();
/// http.Server.call(r); r.timeout` reads the server's `timeout` (0), not
/// undefined.
#[test]
fn object_coerce_receiver_reads_server_state_after_server_call() {
    let prev_http =
        crate::value::JS_NATIVE_HTTP_DISPATCH.swap(http_dispatch as *mut (), Ordering::SeqCst);
    let prev_prop = super::super::class_handles::swap_handle_property_dispatch_for_test(
        server_property as *mut (),
    );
    HTTP_ARMED.with(|a| a.set(true));
    let scope = crate::gc::RuntimeHandleScope::new();
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let receiver = scope.root_nanbox_f64(super::super::js_object_coerce(undefined));
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(b"timeout".as_ptr(), 7));
    // The compiled read site names an interned property atom.
    key.with_const_ptr(|k| unsafe {
        crate::string::property_atom(k);
    });
    let object = scope.root_raw_mut_ptr(
        (receiver.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize
            as *mut super::super::ObjectHeader,
    );
    assert_eq!(
        read_key(&object, &key).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    unsafe {
        js_http_server_construct_with_this(receiver.get_nanbox_f64(), undefined, undefined);
    }
    let (_, kind, admitted) = kind_of(&object);
    assert_eq!(kind, Some(ShapeObjectKind::OrdinaryUnmarkedNativeAlias));
    assert!(!admitted);
    let got = read_key(&object, &key);
    HTTP_ARMED.with(|a| a.set(false));
    crate::value::JS_NATIVE_HTTP_DISPATCH.store(prev_http, Ordering::SeqCst);
    super::super::class_handles::swap_handle_property_dispatch_for_test(prev_prop);
    assert_eq!(
        got.to_bits(),
        0.0_f64.to_bits(),
        "`timeout` must forward to the server (got {:#x})",
        got.to_bits()
    );
}
