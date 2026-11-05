//! #11471 thread-exit regression tests (symbols group).
//!
//! Each test populates one of perry-runtime's process-global address-keyed
//! tables from a short-lived thread through the real entry points, proves the
//! entry exists while the thread lives, joins, and asserts the thread's arena
//! release dropped it. Addresses are re-read through handles at the end of the
//! thread body because an allocation may have moved an owner, and the tables
//! follow a moved owner to its new key.

use perry_runtime::gc::RuntimeHandleScope;
use perry_runtime::value::{js_nanbox_pointer, js_nanbox_string};

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const ADDR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

extern "C" {
    fn js_buffer_set_crypto_key_meta_external(
        addr: usize,
        algo: u8,
        hash: u8,
        kind: u8,
        extractable: u8,
        usages: u32,
        bit_length: u32,
    );
}

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn string_value(text: &str) -> f64 {
    let s = perry_runtime::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    js_nanbox_string(s as i64)
}

fn key(text: &str) -> *const perry_runtime::StringHeader {
    perry_runtime::js_string_from_bytes(text.as_ptr(), text.len() as u32)
}

fn addr_of(value: f64) -> usize {
    (value.to_bits() & ADDR_MASK) as usize
}

/// Is the symbol `(addr, id)` still owned by the current heap?
///
/// #11539: the address alone is not an identity. The dead thread's `Symbol()`
/// header is returned to the system allocator at thread exit, and a
/// concurrently running test's `Symbol()` can be handed the same block and
/// register the same address. An address-only check then reports the live
/// newcomer as the dead symbol (~6 in 1000 runs of this module on Linux, every
/// one showing a live symbol with the next id at the dead one's address).
/// Symbol ids are monotonic and never reissued, so the id pins the symbol.
fn symbol_still_registered(addr: usize, id: u64) -> bool {
    perry_runtime::symbol::registered_symbol_id_for_test(addr) == Some(id)
}

/// Fresh symbol headers are GC cells. Their owning thread frees its heap
/// at exit; another thread must not recognize that former address as live.
#[test]
fn thread_exit_releases_the_threads_fresh_symbol_pointers() {
    let (sym, id) = std::thread::spawn(|| {
        let sym = addr_of(unsafe { perry_runtime::symbol::js_symbol_new(string_value("t11471p")) });
        (
            sym,
            perry_runtime::symbol::registered_symbol_id_for_test(sym),
        )
    })
    .join()
    .unwrap();
    let id = id.expect("Symbol() must be registered while its thread lives");
    assert!(
        !symbol_still_registered(sym, id),
        "a dead thread's Symbol() retained heap ownership"
    );
}

/// #11539 regression: the verdict above must not be fooled by another symbol
/// registered at the dead one's address, and must still go red for the dead
/// symbol itself. Models the reuse with a live symbol standing at an address
/// whose previous (dead) occupant had a different id — exactly what the
/// flaky runs showed — so it needs no allocator cooperation to reproduce.
#[test]
fn symbol_verdict_is_by_identity_not_by_address() {
    let scope = RuntimeHandleScope::new();
    let sym = scope
        .root_nanbox_f64(unsafe { perry_runtime::symbol::js_symbol_new(string_value("t11539")) });
    let addr = addr_of(sym.get_nanbox_f64());
    let live_id = perry_runtime::symbol::registered_symbol_id_for_test(addr)
        .expect("a live Symbol() is registered");
    assert!(
        perry_runtime::symbol::symbol_pointer_registered_for_test(addr),
        "the reused address is registered, so an address-only verdict would fail"
    );
    // A symbol created earlier at this address was necessarily a different
    // one: ids are monotonic, so any earlier occupant's id is smaller.
    let dead_id = live_id.wrapping_sub(1);
    assert!(
        !symbol_still_registered(addr, dead_id),
        "a newcomer at a dead symbol's address was reported as the dead symbol"
    );
    assert!(
        symbol_still_registered(addr, live_id),
        "the verdict must still see a symbol that really is registered"
    );
}

#[test]
fn thread_exit_drops_the_threads_owned_prototype() {
    let alive = std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let arr = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(0));
        let proto = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(0));
        perry_runtime::object::js_object_set_prototype_of(
            js_nanbox_pointer(arr.get_raw_mut_ptr::<u8>() as i64),
            js_nanbox_pointer(proto.get_raw_mut_ptr::<u8>() as i64),
        );
        perry_runtime::object::js_object_get_prototype_of(js_nanbox_pointer(
            arr.get_raw_mut_ptr::<u8>() as i64,
        ))
        .to_bits()
            == js_nanbox_pointer(proto.get_raw_mut_ptr::<u8>() as i64).to_bits()
    })
    .join()
    .unwrap();
    assert!(
        alive,
        "the array must own its prototype while the thread lives"
    );
    std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let fresh = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(0));
        let prototype = perry_runtime::object::js_object_get_prototype_of(js_nanbox_pointer(
            fresh.get_raw_mut_ptr::<u8>() as i64,
        ));
        let key = string_value("__objtables_worker_prototype");
        assert_eq!(
            perry_runtime::object::js_object_get_field_by_name_f64(
                perry_runtime::value::js_nanbox_get_pointer(prototype)
                    as *const perry_runtime::ObjectHeader,
                perry_runtime::value::js_nanbox_get_pointer(key)
                    as *const perry_runtime::StringHeader,
            )
            .to_bits(),
            TAG_UNDEFINED
        );
    })
    .join()
    .unwrap();
}

#[test]
fn thread_exit_releases_the_threads_buffer_own_props() {
    const PROP: &str = "__perry_11471_buffer_own_prop";
    let (addr, alive) = std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let buf = scope.root_raw_mut_ptr(perry_runtime::buffer::js_buffer_alloc(4, 0));
        let value = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(0));
        let v = js_nanbox_pointer(value.get_raw_mut_ptr::<u8>() as i64);
        let addr = buf.get_raw_mut_ptr::<u8>() as usize;
        perry_runtime::buffer::buffer_set_own_prop(addr, PROP, v);
        (
            addr,
            perry_runtime::buffer::buffer_get_own_prop(addr, PROP).is_some(),
        )
    })
    .join()
    .unwrap();
    assert!(alive, "the expando must exist while its thread lives");
    assert!(
        perry_runtime::buffer::buffer_get_own_prop(addr, PROP).is_none(),
        "a dead thread's buffer expando outlived its heap"
    );
}

/// Record CryptoKey metadata while the exiting thread still owns its heap.
/// Checking after join could mistake a new allocation at the same address
/// for metadata left behind by the dead thread.
static EXTERNAL_BUFFER_EXIT_PROBE: std::sync::Mutex<(usize, Option<bool>)> =
    std::sync::Mutex::new((0, None));

fn record_external_buffer_release(freed: &perry_runtime::arena::thread_exit::FreedRanges) {
    let mut probe = EXTERNAL_BUFFER_EXIT_PROBE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (key, seen) = *probe;
    if key != 0 && seen.is_none() && freed.contains(key) {
        probe.1 = Some(perry_runtime::buffer::external_registries_hold_for_test(
            key,
        ));
    }
}

fn external_buffer_release_verdict(seen: Option<bool>) -> Result<(), &'static str> {
    match seen {
        Some(false) => Ok(()),
        Some(true) => Err("EXTERNAL_CRYPTO_KEY_META_REGISTRY outlived the thread"),
        None => Err("the thread's release never reported the buffer's range"),
    }
}

#[test]
fn thread_exit_releases_the_threads_external_buffer_registrations() {
    let alive = std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let buf = scope.root_raw_mut_ptr(
            perry_runtime::JSValue::from_bits(
                perry_runtime::buffer::bytes::from_slice(
                    perry_runtime::buffer::bytes::Brand::CryptoKey,
                    &[0; 16],
                )
                .to_bits(),
            )
            .as_pointer::<u8>()
            .cast_mut(),
        );
        let addr = buf.get_raw_mut_ptr::<u8>() as usize;
        // Registration installs the cleanup hook before the observing hook.
        unsafe { js_buffer_set_crypto_key_meta_external(addr, 1, 0, 1, 1, 0, 0) };
        *EXTERNAL_BUFFER_EXIT_PROBE.lock().unwrap() = (addr, None);
        perry_runtime::arena::thread_exit::register_thread_exit_range_hook(
            record_external_buffer_release,
        );
        perry_runtime::buffer::external_registries_hold_for_test(addr)
    })
    .join()
    .unwrap();
    let seen = std::mem::replace(&mut *EXTERNAL_BUFFER_EXIT_PROBE.lock().unwrap(), (0, None)).1;
    assert!(alive, "the registration must exist while its thread lives");
    if let Err(table) = external_buffer_release_verdict(seen) {
        panic!("{table}");
    }
}

#[test]
fn external_buffer_verdict_is_taken_at_release_not_by_address() {
    let scope = RuntimeHandleScope::new();
    let buf = scope.root_raw_mut_ptr(
        perry_runtime::JSValue::from_bits(
            perry_runtime::buffer::bytes::from_slice(
                perry_runtime::buffer::bytes::Brand::CryptoKey,
                &[0; 16],
            )
            .to_bits(),
        )
        .as_pointer::<u8>()
        .cast_mut(),
    );
    let addr = buf.get_raw_mut_ptr::<u8>() as usize;
    unsafe { js_buffer_set_crypto_key_meta_external(addr, 1, 0, 1, 1, 0, 0) };
    assert!(
        perry_runtime::buffer::external_registries_hold_for_test(addr),
        "the newcomer is registered, so an address-only check would fail"
    );
    assert_eq!(
        external_buffer_release_verdict(Some(false)),
        Ok(()),
        "a newcomer at a released address was reported as the dead buffer"
    );
    assert_eq!(
        external_buffer_release_verdict(Some(true)),
        Err("EXTERNAL_CRYPTO_KEY_META_REGISTRY outlived the thread")
    );
    assert!(
        external_buffer_release_verdict(None).is_err(),
        "a release that never reported the range must not pass"
    );
}

#[test]
fn thread_exit_releases_the_threads_dom_exceptions() {
    // Initialize the observing heap before the worker releases its blocks.
    // The brand is now in the cell, not an address registry: initializing
    // this heap after join can reuse the worker's block and register its
    // stale bytes as our own arena (notably with alloc-mimalloc enabled).
    let scope = RuntimeHandleScope::new();
    let _observer = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(0));
    let (err, alive) = std::thread::spawn(|| {
        let err =
            perry_runtime::event_target::js_dom_exception_new(undefined(), undefined()) as usize;
        (
            err,
            perry_runtime::event_target::dom_exception_has_error_brand_for_test(err),
        )
    })
    .join()
    .unwrap();
    assert!(
        alive,
        "new DOMException() must be recorded while its thread lives"
    );
    assert!(
        !perry_runtime::event_target::dom_exception_has_error_brand_for_test(err),
        "a dead thread's DOMException address outlived its heap"
    );
}

/// Script metadata is reached through the owner's private bag. The main
/// thread must not find the exited thread's cells in its tracked heap.
#[test]
fn thread_exit_releases_the_threads_vm_script_entry() {
    let (script, alive) = std::thread::spawn(|| {
        use perry_runtime::node_vm as vm;
        let scope = RuntimeHandleScope::new();
        let script =
            scope.root_nanbox_f64(vm::js_vm_create_script(string_value("1 + 1"), undefined()));
        let script = addr_of(script.get_nanbox_f64());
        (script, vm::vm_owner_entries_for_test(script).0)
    })
    .join()
    .unwrap();
    assert!(
        alive,
        "new vm.Script() must be recorded while its thread lives"
    );
    assert!(
        !perry_runtime::node_vm::vm_owner_entries_for_test(script).0,
        "a dead thread's vm.Script entry outlived its heap"
    );
}

#[test]
fn thread_exit_releases_the_threads_message_ports() {
    let (ports, alive) = std::thread::spawn(|| {
        use perry_runtime::messaging as m;
        perry_runtime::feature_hooks::js_runtime_install_messaging();
        let scope = RuntimeHandleScope::new();
        let channel = scope.root_nanbox_f64(m::same_thread_message_channel_new_for_test());
        let port = |name: &str| {
            let obj = addr_of(channel.get_nanbox_f64()) as *const perry_runtime::ObjectHeader;
            addr_of(perry_runtime::object::js_object_get_field_by_name_f64(
                obj,
                key(name),
            ))
        };
        let ports = [port("port1"), port("port2")];
        let alive = ports.map(m::port_state_registered_for_test);
        (ports, alive)
    })
    .join()
    .unwrap();
    assert_eq!(
        alive,
        [true, true],
        "both ports must be registered while their thread lives"
    );
    for port in ports {
        assert!(
            !perry_runtime::messaging::port_state_registered_for_test(port),
            "a dead thread's MessagePort state outlived its heap"
        );
    }
}
