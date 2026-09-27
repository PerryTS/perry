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
    static PERRY_U8_INLINE_CACHE: [std::sync::atomic::AtomicU64; 64];
}

extern "C" fn probe_thunk(_closure: *const perry_runtime::ClosureHeader) -> f64 {
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

fn u8_cache_holds(addr: usize) -> bool {
    let slot = (addr >> 3) & 63;
    unsafe { PERRY_U8_INLINE_CACHE[slot].load(std::sync::atomic::Ordering::Relaxed) == addr as u64 }
}

#[test]
fn thread_exit_releases_the_threads_symbol_side_table_entries() {
    const CLASS_ID: u32 = 0x0B11_4711;
    let ((owner, sym), alive) = std::thread::spawn(|| {
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
            probe_thunk as *const u8,
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
        // static [sym] = [] on a (process-global) class id (CLASS_STATIC_SYMBOLS).
        unsafe {
            s::js_class_register_static_symbol(CLASS_ID, sym.get_nanbox_f64(), value_value())
        };

        let owner = obj.get_raw_mut_ptr::<u8>() as usize;
        let (sym, sym2, sym3) = (
            addr_of(sym.get_nanbox_f64()),
            addr_of(sym2.get_nanbox_f64()),
            addr_of(sym3.get_nanbox_f64()),
        );
        let alive = [
            s::symbol_property_tables_hold_for_test(owner, sym).0,
            s::symbol_property_tables_hold_for_test(owner, sym2).1,
            s::symbol_accessor_held_for_test(owner, sym3),
            s::class_static_symbol_held_for_test(CLASS_ID, sym),
        ];
        ((owner, [sym, sym2, sym3]), alive)
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
        !s::class_static_symbol_held_for_test(CLASS_ID, sym[0]),
        "a dead thread's class-static symbol member outlived its heap"
    );
}

/// `SYMBOL_POINTERS`: `Symbol()` headers are `gc_malloc`'d, not arena cells;
/// `MallocState`'s TLS destructor (`gc/malloc.rs`) reports their freed ranges
/// to `arena::thread_exit::release_freed_ranges` before freeing them.
#[test]
fn thread_exit_releases_the_threads_fresh_symbol_pointers() {
    let (sym, alive) = std::thread::spawn(|| {
        let sym = addr_of(unsafe { perry_runtime::symbol::js_symbol_new(string_value("t11471p")) });
        (
            sym,
            perry_runtime::symbol::symbol_pointer_registered_for_test(sym),
        )
    })
    .join()
    .unwrap();
    assert!(alive, "Symbol() must be registered while its thread lives");
    assert!(
        !perry_runtime::symbol::symbol_pointer_registered_for_test(sym),
        "a dead thread's Symbol() stayed in SYMBOL_POINTERS"
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

#[test]
fn thread_exit_releases_the_threads_external_buffer_registrations() {
    let (addr, alive) = std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let buf = scope.root_raw_mut_ptr(perry_runtime::buffer::js_buffer_alloc(16, 0));
        let addr = buf.get_raw_mut_ptr::<u8>() as usize;
        // webcrypto's CryptoKey registration: all three external registries.
        unsafe { js_buffer_mark_as_crypto_key_external(addr, 1, 0, 1, 1, 0, 0) };
        // The codegen inline-read slow arm primes PERRY_U8_INLINE_CACHE.
        unsafe { js_u8_buffer_read_f64(addr as *const u8, 0) };
        let cached = u8_cache_holds(addr);
        let alive = [
            perry_runtime::buffer::is_external_buffer(addr),
            perry_runtime::buffer::is_uint8array_buffer(addr),
            perry_runtime::buffer::crypto_key_meta(addr).is_some(),
            cached,
        ];
        (addr, alive)
    })
    .join()
    .unwrap();
    assert_eq!(
        alive, [true; 4],
        "every registration must exist while its thread lives"
    );
    assert!(
        !perry_runtime::buffer::is_external_buffer(addr),
        "EXTERNAL_BUFFER_REGISTRY outlived the thread"
    );
    assert!(
        !perry_runtime::buffer::is_uint8array_buffer(addr),
        "EXTERNAL_UINT8ARRAY_REGISTRY outlived the thread"
    );
    assert!(
        perry_runtime::buffer::crypto_key_meta(addr).is_none(),
        "EXTERNAL_CRYPTO_KEY_META_REGISTRY outlived the thread"
    );
    assert!(
        !u8_cache_holds(addr),
        "PERRY_U8_INLINE_CACHE outlived the thread"
    );
}

#[test]
fn thread_exit_releases_the_threads_dom_exceptions() {
    let (err, alive) = std::thread::spawn(|| {
        let err =
            perry_runtime::event_target::js_dom_exception_new(undefined(), undefined()) as usize;
        (
            err,
            perry_runtime::event_target::dom_exception_error_registered_for_test(err),
        )
    })
    .join()
    .unwrap();
    assert!(
        alive,
        "new DOMException() must be recorded while its thread lives"
    );
    assert!(
        !perry_runtime::event_target::dom_exception_error_registered_for_test(err),
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
