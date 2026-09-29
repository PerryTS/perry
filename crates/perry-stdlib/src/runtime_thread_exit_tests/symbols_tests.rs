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
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const ADDR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

extern "C" {
    fn js_buffer_mark_as_crypto_key_external(
        addr: usize,
        algo: u8,
        hash: u8,
        kind: u8,
        extractable: u8,
        usages: u32,
        bit_length: u32,
    );
    fn perry_thread_exit_probe_object_prototype_recorded(owner: usize) -> bool;
    fn js_u8_buffer_read_f64(target: *const u8, index: i32) -> f64;
}

extern "C" fn probe_thunk(
    _closure: *const perry_runtime::ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    0.0
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

#[test]
fn thread_exit_releases_the_threads_symbol_side_table_entries() {
    const STATIC_SYMBOL_CLASS: u32 = 0x0B11_4711;
    let ((owner, class_owner, sym), alive) = std::thread::spawn(|| {
        use perry_runtime::symbol as s;
        let scope = RuntimeHandleScope::new();
        let sym = scope.root_nanbox_f64(unsafe { s::js_symbol_new(string_value("t11471")) });
        let obj = scope.root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 0));
        let obj_value = || js_nanbox_pointer(obj.get_raw_mut_ptr::<u8>() as i64);
        let value = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(0));
        let value_value = || js_nanbox_pointer(value.get_raw_mut_ptr::<u8>() as i64);
        // obj[sym] = [] (SYMBOL_PROPERTIES).
        unsafe {
            s::js_object_set_symbol_property(obj_value(), sym.get_nanbox_f64(), value_value())
        };
        // Object.defineProperty(obj, sym2, { value: [], writable: false })
        // (SYMBOL_PROPERTY_ATTRS).
        let sym2 = scope.root_nanbox_f64(unsafe { s::js_symbol_new(string_value("t11471b")) });
        let desc = scope.root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 0));
        perry_runtime::js_object_set_field_by_name(
            desc.get_raw_mut_ptr(),
            key("value"),
            value_value(),
        );
        perry_runtime::js_object_set_field_by_name(
            desc.get_raw_mut_ptr(),
            key("writable"),
            f64::from_bits(TAG_FALSE),
        );
        perry_runtime::object::js_object_define_property(
            obj_value(),
            sym2.get_nanbox_f64(),
            js_nanbox_pointer(desc.get_raw_mut_ptr::<u8>() as i64),
        );
        // Object.defineProperty(obj, sym3, { get }) (SYMBOL_ACCESSOR_PROPERTIES).
        let sym3 = scope.root_nanbox_f64(unsafe { s::js_symbol_new(string_value("t11471c")) });
        let getter = scope.root_raw_mut_ptr(perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(probe_thunk, 0),
            0,
        ));
        let accessor = scope.root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 0));
        perry_runtime::js_object_set_field_by_name(
            accessor.get_raw_mut_ptr(),
            key("get"),
            js_nanbox_pointer(getter.get_raw_mut_ptr::<u8>() as i64),
        );
        perry_runtime::object::js_object_define_property(
            obj_value(),
            sym3.get_nanbox_f64(),
            js_nanbox_pointer(accessor.get_raw_mut_ptr::<u8>() as i64),
        );
        // static [sym] = [] on a class id: an own symbol property of the class's
        // function object, which this thread's agent mints in its own heap.
        unsafe {
            s::js_class_register_static_symbol(
                STATIC_SYMBOL_CLASS,
                sym.get_nanbox_f64(),
                value_value(),
            )
        };

        let owner = obj.get_raw_mut_ptr::<u8>() as usize;
        let class_owner = s::class_static_symbol_owner_for_test(STATIC_SYMBOL_CLASS);
        let (sym, sym2, sym3) = (
            addr_of(sym.get_nanbox_f64()),
            addr_of(sym2.get_nanbox_f64()),
            addr_of(sym3.get_nanbox_f64()),
        );
        let alive = [
            s::symbol_property_tables_hold_for_test(owner, sym).0,
            s::symbol_property_tables_hold_for_test(owner, sym2).1,
            s::symbol_accessor_held_for_test(owner, sym3),
            s::symbol_property_tables_hold_for_test(class_owner, sym).0,
        ];
        ((owner, class_owner, [sym, sym2, sym3]), alive)
    })
    .join()
    .unwrap();

    use perry_runtime::symbol as s;
    assert_eq!(
        alive, [true; 4],
        "every entry must exist while its thread lives"
    );
    assert!(
        !s::symbol_property_tables_hold_for_test(owner, sym[0]).0,
        "a dead thread's obj[sym] record outlived its heap"
    );
    assert!(
        !s::symbol_property_tables_hold_for_test(owner, sym[1]).1,
        "a dead thread's symbol property attrs outlived its heap"
    );
    assert!(
        !s::symbol_accessor_held_for_test(owner, sym[2]),
        "a dead thread's symbol accessor outlived its heap"
    );
    assert!(
        !s::symbol_property_tables_hold_for_test(class_owner, sym[0]).0,
        "a dead thread's class-static symbol member outlived its heap"
    );
}

/// Is the symbol `(addr, id)` still in `SYMBOL_POINTERS`?
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

/// `SYMBOL_POINTERS`: `Symbol()` headers are `gc_malloc`'d, not arena cells;
/// `MallocState`'s TLS destructor (`gc/malloc.rs`) reports their freed ranges
/// to `arena::thread_exit::release_freed_ranges` before freeing them.
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
        "a dead thread's Symbol() stayed in SYMBOL_POINTERS"
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
fn thread_exit_releases_the_threads_residual_prototype_entry() {
    let (owner, alive) = std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let arr = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(0));
        let proto = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(0));
        perry_runtime::object::js_object_set_prototype_of(
            js_nanbox_pointer(arr.get_raw_mut_ptr::<u8>() as i64),
            js_nanbox_pointer(proto.get_raw_mut_ptr::<u8>() as i64),
        );
        let owner = arr.get_raw_mut_ptr::<u8>() as usize;
        (owner, unsafe {
            perry_thread_exit_probe_object_prototype_recorded(owner)
        })
    })
    .join()
    .unwrap();
    assert!(
        alive,
        "Object.setPrototypeOf(array) must record a residual entry"
    );
    assert!(
        !unsafe { perry_thread_exit_probe_object_prototype_recorded(owner) },
        "a dead thread's residual prototype entry outlived its heap"
    );
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

/// Membership of the CryptoKey buffer `key` in the three process-global
/// external-buffer registries, and of the byte view `view` in
/// `PERRY_U8_INLINE_CACHE`, in that order. Reads no thread-local, so the
/// thread-exit probe below may call it.
///
/// Two addresses because since #11589 key material is never admitted to the
/// inline-access cache (a key is not integer-indexed), so the key buffer can
/// no longer witness the cache's release; an ordinary `Buffer` on the same
/// thread does.
fn external_buffer_registrations(key: usize, view: usize) -> [bool; 4] {
    let [ext, u8a, meta] = perry_runtime::buffer::external_registries_hold_for_test(key);
    [
        ext,
        u8a,
        meta,
        perry_runtime::buffer::u8_inline_cache_holds_for_test(view),
    ]
}

/// #11547: the external-buffer test's buffer address, and what the tables held
/// for it when its thread's arena was released.
///
/// Checking the tables after `join` cannot tell the dead buffer from a new one.
/// The dying thread's arena goes back to the allocator, and a concurrent test
/// (`thread_exit_releases_the_threads_crypto_key_entries` registers a CryptoKey
/// Buffer through the same `js_buffer_mark_as_crypto_key_external`) can be
/// given the same address and register it again. So the verdict is taken
/// INSIDE the release instead: [`record_external_buffer_release`] is a
/// thread-exit range hook that runs after the external-buffer registries' own
/// hook, while the block is still owned by the exiting thread and so cannot
/// belong to anyone else.
///
/// Holds `(key, view, seen)`: the CryptoKey buffer, the cache-admitted byte
/// view, and the verdict.
#[allow(clippy::type_complexity)]
static EXTERNAL_BUFFER_EXIT_PROBE: std::sync::Mutex<(usize, usize, Option<[bool; 4]>)> =
    std::sync::Mutex::new((0, 0, None));

fn record_external_buffer_release(freed: &perry_runtime::arena::thread_exit::FreedRanges) {
    let mut probe = EXTERNAL_BUFFER_EXIT_PROBE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (key, view, seen) = *probe;
    // First release only: once the addresses are back with the allocator, a
    // later tenant's own thread exit reports them again. Both live on the same
    // thread, so its one release covers both.
    if key != 0 && seen.is_none() && freed.contains(key) && freed.contains(view) {
        probe.2 = Some(external_buffer_registrations(key, view));
    }
}

/// Verdict on what [`record_external_buffer_release`] saw: `Err` names the
/// first table that still held the dead buffer when its thread was released.
fn external_buffer_release_verdict(seen: Option<[bool; 4]>) -> Result<(), &'static str> {
    const TABLES: [&str; 4] = [
        "EXTERNAL_BUFFER_REGISTRY outlived the thread",
        "EXTERNAL_UINT8ARRAY_REGISTRY outlived the thread",
        "EXTERNAL_CRYPTO_KEY_META_REGISTRY outlived the thread",
        "PERRY_U8_INLINE_CACHE outlived the thread",
    ];
    let seen = seen.ok_or("the thread's release never reported the buffer's range")?;
    match seen.iter().position(|&held| held) {
        Some(i) => Err(TABLES[i]),
        None => Ok(()),
    }
}

#[test]
fn thread_exit_releases_the_threads_external_buffer_registrations() {
    let alive = std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let buf = scope.root_raw_mut_ptr(perry_runtime::buffer::js_buffer_alloc(16, 0));
        let addr = buf.get_raw_mut_ptr::<u8>() as usize;
        // webcrypto's CryptoKey registration: all three external registries.
        // This also registers their thread-exit hook, so it runs before the
        // probe registered below.
        unsafe { js_buffer_mark_as_crypto_key_external(addr, 1, 0, 1, 1, 0, 0) };
        // The codegen inline-read slow arm primes PERRY_U8_INLINE_CACHE — for
        // a byte view. It must refuse the key (#11589: key material is not
        // integer-indexed), so an ordinary Buffer carries the cache probe.
        let view = scope.root_raw_mut_ptr(perry_runtime::buffer::js_buffer_alloc(16, 0));
        let view_addr = view.get_raw_mut_ptr::<u8>() as usize;
        unsafe { js_u8_buffer_read_f64(addr as *const u8, 0) };
        unsafe { js_u8_buffer_read_f64(view_addr as *const u8, 0) };
        let key_admitted = perry_runtime::buffer::u8_inline_cache_holds_for_test(addr);
        *EXTERNAL_BUFFER_EXIT_PROBE.lock().unwrap() = (addr, view_addr, None);
        perry_runtime::arena::thread_exit::register_thread_exit_range_hook(
            record_external_buffer_release,
        );
        (external_buffer_registrations(addr, view_addr), key_admitted)
    })
    .join()
    .unwrap();
    let (alive, key_admitted) = alive;
    let seen = std::mem::replace(
        &mut *EXTERNAL_BUFFER_EXIT_PROBE.lock().unwrap(),
        (0, 0, None),
    )
    .2;
    assert!(
        !key_admitted,
        "key material must never enter the inline element-access cache"
    );
    assert_eq!(
        alive, [true; 4],
        "every registration must exist while its thread lives"
    );
    if let Err(table) = external_buffer_release_verdict(seen) {
        panic!("{table}");
    }
}

/// #11547 regression: a buffer registered at the dead one's address after the
/// release must not fail the verdict, and the verdict must still fail when
/// the release left an entry behind or never saw the range. The newcomer is a
/// real registered buffer on this thread, which is exactly what the
/// address-only check used to report as "outlived".
#[test]
fn external_buffer_verdict_is_taken_at_release_not_by_address() {
    let scope = RuntimeHandleScope::new();
    let buf = scope.root_raw_mut_ptr(perry_runtime::buffer::js_buffer_alloc(16, 0));
    let addr = buf.get_raw_mut_ptr::<u8>() as usize;
    unsafe { js_buffer_mark_as_crypto_key_external(addr, 1, 0, 1, 1, 0, 0) };
    assert!(
        perry_runtime::buffer::is_external_buffer(addr),
        "the newcomer is registered, so an address-only check would fail"
    );
    assert_eq!(
        external_buffer_release_verdict(Some([false; 4])),
        Ok(()),
        "a newcomer at a released address was reported as the dead buffer"
    );
    assert_eq!(
        external_buffer_release_verdict(Some([true, false, false, false])),
        Err("EXTERNAL_BUFFER_REGISTRY outlived the thread")
    );
    assert_eq!(
        external_buffer_release_verdict(Some([false, false, false, true])),
        Err("PERRY_U8_INLINE_CACHE outlived the thread")
    );
    assert!(
        external_buffer_release_verdict(None).is_err(),
        "a release that never reported the range must not pass"
    );
}

#[test]
fn thread_exit_releases_the_threads_dom_exceptions() {
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

/// `VM_SCRIPTS` only: `VM_COMPILED_FUNCTION_SOURCES` is filled by
/// `vm.compileFunction`, which needs perry-runtime's `dyn-eval` feature, off in
/// this crate's test build (`default-features = false`). The same hook clears
/// both maps by the same owner-address rule.
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
