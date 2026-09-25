//! #11286: the handle dispatch-extension tables must hold every registrant.
//!
//! They used to be fixed four-slot arrays whose overflow path overwrote the
//! last slot, so a fifth registrant silently evicted the fourth. Five crates
//! register a method extension (perry-ext-http's server and client halves,
//! perry-ext-net, perry-ext-ws, perry-ext-nodemailer), so a program linking
//! all of them lost one crate's dynamic dispatch depending on first-use order.

use super::*;
use std::sync::atomic::AtomicU64;

/// Handle ids no other runtime test or real registry hands out, so these
/// test-only extensions decline everything else and stay inert in the
/// process-global tables after the test finishes.
const BASE: i64 = 0x0011_2860_0000;
/// More registrants than the old fixed capacity (4), with headroom.
const N: i64 = 9;

unsafe extern "C" fn method_ext<const K: i64>(
    handle: i64,
    _name_ptr: *const u8,
    _name_len: usize,
    _args_ptr: *const f64,
    _args_len: usize,
    out: *mut f64,
) -> i32 {
    if handle != BASE + K {
        return 0;
    }
    *out = (100 + K) as f64;
    1
}

unsafe extern "C" fn property_ext<const K: i64>(
    handle: i64,
    _name_ptr: *const u8,
    _name_len: usize,
    out: *mut f64,
) -> i32 {
    if handle != BASE + K {
        return 0;
    }
    *out = (200 + K) as f64;
    1
}

per_test_global! {
    // Per test thread, so no sibling test can observe or disturb it.
    static SET_HITS: [AtomicU64; N as usize] = [const { AtomicU64::new(0) }; N as usize];
}

unsafe extern "C" fn property_set_ext<const K: i64>(
    handle: i64,
    _name_ptr: *const u8,
    _name_len: usize,
    value: f64,
) -> i32 {
    if handle != BASE + K {
        return 0;
    }
    SET_HITS[K as usize].store(value.to_bits(), Ordering::Release);
    1
}

macro_rules! for_each_k {
    ($m:ident: $t:ty) => {{
        let fns: [$t; N as usize] = [
            $m::<0>, $m::<1>, $m::<2>, $m::<3>, $m::<4>, $m::<5>, $m::<6>, $m::<7>, $m::<8>,
        ];
        fns
    }};
}

#[test]
fn extension_table_keeps_every_registrant_in_order() {
    let table = ExtensionTable::new();
    assert!(table.is_empty());
    assert!(table.snapshot().is_empty());
    let fns: Vec<*mut ()> = for_each_k!(method_ext: HandleMethodDispatchExtensionFn)
        .iter()
        .map(|&f| f as *mut ())
        .collect();
    for &f in &fns {
        table.register(f);
    }
    // Re-registering is idempotent and never displaces anything.
    table.register(fns[0]);
    table.register(fns[N as usize - 1]);
    table.register(ptr::null_mut());
    assert_eq!(
        table.snapshot(),
        fns,
        "registration order is dispatch order"
    );
    assert!(!table.is_empty());
}

#[test]
fn global_tables_dispatch_to_more_than_four_extensions() {
    let methods = for_each_k!(method_ext: HandleMethodDispatchExtensionFn);
    let props = for_each_k!(property_ext: HandlePropertyDispatchExtensionFn);
    let sets = for_each_k!(property_set_ext: HandlePropertySetDispatchExtensionFn);
    for k in 0..N as usize {
        unsafe {
            js_register_handle_method_dispatch_extension(methods[k]);
            js_register_handle_property_dispatch_extension(props[k]);
            js_register_handle_property_set_dispatch_extension(sets[k]);
        }
    }
    let method = handle_method_dispatch().expect("method extensions registered");
    let property = handle_property_dispatch().expect("property extensions registered");
    let set = handle_property_set_dispatch().expect("property-set extensions registered");
    let name = b"probe";
    for k in 0..N {
        let handle = BASE + k;
        // Every registrant must answer for its own handle — before #11286 the
        // fifth registration overwrote the fourth, so k == 3 lost dispatch.
        let v = unsafe { method(handle, name.as_ptr(), name.len(), ptr::null(), 0) };
        assert_eq!(v, (100 + k) as f64, "method extension #{k} lost");
        let v = unsafe { property(handle, name.as_ptr(), name.len()) };
        assert_eq!(v, (200 + k) as f64, "property extension #{k} lost");
        let value = (300 + k) as f64;
        unsafe { set(handle, name.as_ptr(), name.len(), value) };
        assert_eq!(
            SET_HITS[k as usize].load(Ordering::Acquire),
            value.to_bits(),
            "property-set extension #{k} lost"
        );
    }
}
