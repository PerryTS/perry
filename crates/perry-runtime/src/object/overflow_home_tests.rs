//! Table operations on a meta record no ObjectHeader carries (FR2).
use super::*;

unsafe fn atom(name: &str) -> u64 {
    let hash = crate::object::key_bytes_hash(name.as_ptr(), name.len());
    let key = crate::string::js_string_pool_atom(name.as_ptr(), name.len() as u32, hash, 0);
    crate::JSValue::string_ptr(key).bits()
}

fn run(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

unsafe fn live_keys(owner: usize) -> Vec<u64> {
    let t = MetaHome::table(owner);
    (0..(*t).length)
        .map(|i| *key_at(t, i))
        .filter(|&k| k != TAG_HOLE)
        .collect()
}

unsafe fn get(owner: usize, key: u64) -> Option<u64> {
    let t = MetaHome::table(owner);
    if t.is_null() {
        return None;
    }
    let i = find(t, key)?;
    Some(*value_at(t, i))
}

unsafe fn names(prefix: &str, n: usize) -> Vec<u64> {
    (0..n).map(|i| atom(&format!("{prefix}{i}"))).collect()
}

#[test]
fn meta_home_appends_looks_up_and_grows_in_insertion_order() {
    run(|| unsafe {
        crate::gc::initialize_runtime_fixture();
        let _no_move = crate::gc::GcSuppressScope::new();
        let o = meta_home_new() as usize;
        assert!(MetaHome::table(o).is_null());
        let keys = names("mh_grow_", 40);
        for (i, &k) in keys.iter().enumerate() {
            insert_key_bits_in::<true>(o, k, (i as f64).to_bits(), 0);
        }
        let t = MetaHome::table(o);
        assert!(is_table(t.cast()));
        assert_eq!((*t).fixed_prefix, 0);
        assert!((*t).capacity >= 40);
        assert_eq!(live_keys(o), keys);
        for (i, &k) in keys.iter().enumerate() {
            assert_eq!(get(o, k), Some((i as f64).to_bits()));
        }
        // An overwrite keeps the entry's place.
        insert_key_bits_in::<true>(o, keys[3], 99f64.to_bits(), 0);
        assert_eq!(live_keys(o), keys);
        assert_eq!(get(o, keys[3]), Some(99f64.to_bits()));
        let t = MetaHome::table(o);
        assert!((0..(*t).length).all(|i| *attrs_at(t, i) & SLOT_REF == 0));
        assert_eq!(get(o, atom("mh_grow_absent")), None);
    });
}

#[test]
fn meta_home_delete_tombstones_and_readd_appends_at_the_end() {
    run(|| unsafe {
        crate::gc::initialize_runtime_fixture();
        let _no_move = crate::gc::GcSuppressScope::new();
        let o = meta_home_new() as usize;
        let keys = names("mh_del_", 30);
        for (i, &k) in keys.iter().enumerate() {
            insert_key_bits_in::<true>(o, k, (i as f64).to_bits(), 0);
        }
        assert!(delete_key_bits_in::<true>(o, keys[1]));
        assert_eq!(get(o, keys[1]), None);
        assert!(delete_key_bits_in::<true>(o, atom("mh_del_absent")));
        insert_key_bits_in::<true>(o, keys[1], 7f64.to_bits(), 0);
        let mut want: Vec<u64> = keys.iter().copied().filter(|&k| k != keys[1]).collect();
        want.push(keys[1]);
        assert_eq!(live_keys(o), want);
        // Tombstones that dominate are compacted on the next append, order kept.
        for &k in &keys[2..24] {
            assert!(delete_key_bits_in::<true>(o, k));
        }
        let extra = atom("mh_del_extra");
        insert_key_bits_in::<true>(o, extra, 1f64.to_bits(), 0);
        let t = MetaHome::table(o);
        assert_eq!((*t).deleted, 0);
        let mut want: Vec<u64> = vec![keys[0]];
        want.extend_from_slice(&keys[24..]);
        want.push(keys[1]);
        want.push(extra);
        assert_eq!(live_keys(o), want);
        assert_eq!((*t).length as usize, want.len());
        for &k in &want {
            assert!(get(o, k).is_some());
        }
    });
}

#[test]
fn meta_home_attributes_accessors_and_symbols() {
    run(|| unsafe {
        crate::gc::initialize_runtime_fixture();
        let _no_move = crate::gc::GcSuppressScope::new();
        let o = meta_home_new() as usize;
        let ro = atom("mh_attr_ro");
        let fixed = atom("mh_attr_fixed");
        let acc = atom("mh_attr_acc");
        let sym = crate::symbol::js_symbol_new_empty().to_bits();
        insert_key_bits_in::<true>(
            o,
            ro,
            1f64.to_bits(),
            crate::object::key_attrs::ENTRY_NON_WRITABLE,
        );
        insert_key_bits_in::<true>(
            o,
            fixed,
            2f64.to_bits(),
            crate::object::key_attrs::ENTRY_NON_CONFIGURABLE,
        );
        let ai = insert_key_bits_in::<true>(o, acc, 3f64.to_bits(), 0);
        insert_key_bits_in::<true>(o, sym, 4f64.to_bits(), 0);
        let t = MetaHome::table(o);
        set_entry_attrs_in::<true>(o, t, ai, crate::object::key_attrs::ENTRY_ACCESSOR);
        let t = MetaHome::table(o);
        let at = |k: u64| *attrs_at(t, find(t, k).unwrap()) as u8;
        assert_eq!(at(ro), crate::object::key_attrs::ENTRY_NON_WRITABLE);
        assert_eq!(at(acc), crate::object::key_attrs::ENTRY_ACCESSOR);
        assert!(crate::object::key_attrs::key_is_accessor_at(t.cast(), ai));
        assert_eq!(get(o, sym), Some(4f64.to_bits()));
        // A non-configurable entry refuses delete and stays.
        assert!(!delete_key_bits_in::<true>(o, fixed));
        assert_eq!(get(o, fixed), Some(2f64.to_bits()));
        assert_eq!(live_keys(o), vec![ro, fixed, acc, sym]);
    });
}

#[test]
fn meta_home_table_survives_minor_and_full_collection() {
    run(|| unsafe {
        crate::gc::initialize_runtime_fixture();
        let scope = crate::gc::RuntimeHandleScope::new();
        let home = scope.root_raw_mut_ptr(meta_home_new());
        let keys = names("mh_gc_", 20);
        for (i, &k) in keys.iter().enumerate() {
            let o = home.get_raw_mut_ptr::<u8>() as usize;
            insert_key_bits_in::<true>(o, k, (i as f64).to_bits(), 0);
        }
        crate::gc::gc_collect_minor();
        crate::gc::js_gc_collect();
        crate::gc::gc_collect_minor();
        let o = home.get_raw_mut_ptr::<u8>() as usize;
        let t = MetaHome::table(o);
        assert!(is_table(t.cast()));
        assert_eq!(live_keys(o), keys);
        for (i, &k) in keys.iter().enumerate() {
            assert_eq!(get(o, k), Some((i as f64).to_bits()));
        }
    });
}

#[test]
fn meta_home_reads_own_entries_and_refuses_holes() {
    run(|| unsafe {
        crate::gc::initialize_runtime_fixture();
        let _no_move = crate::gc::GcSuppressScope::new();
        let o = meta_home_new() as usize;
        assert!(own_get_key_in::<true>(o, atom("mh_rd_a")).is_none());
        let keys = names("mh_rd_", 12);
        for (i, &k) in keys.iter().enumerate() {
            insert_key_bits_in::<true>(o, k, (i as f64).to_bits(), 0);
        }
        let t = MetaHome::table(o);
        for (i, &k) in keys.iter().enumerate() {
            let slot = find(t, k).unwrap();
            assert_eq!(get_from_in::<true>(t, o, slot), Some((i as f64).to_bits()));
            assert_eq!(
                own_get_key_in::<true>(o, k).map(|v| v.bits()),
                Some((i as f64).to_bits())
            );
        }
        assert!(own_get_key_in::<true>(o, atom("mh_rd_absent")).is_none());
        assert!(delete_key_bits_in::<true>(o, keys[3]));
        assert!(own_get_key_in::<true>(o, keys[3]).is_none());
        assert_eq!(get_from_in::<true>(t, o, (*t).length), None);
        insert_key_bits_in::<true>(o, keys[4], TAG_HOLE, 0);
        assert!(
            own_get_key_in::<true>(o, keys[4]).is_none(),
            "a hole value is refused"
        );
    });
}

#[test]
fn meta_home_routed_stores_append_then_overwrite_in_place() {
    run(|| unsafe {
        crate::gc::initialize_runtime_fixture();
        let _no_move = crate::gc::GcSuppressScope::new();
        let o = meta_home_new() as usize;
        let keys = names("mh_st_", 10);
        for (i, &k) in keys.iter().enumerate() {
            let s = crate::JSValue::from_bits(k).as_string_ptr();
            store_routed_in::<true>(o, s, i as f64);
        }
        assert_eq!(live_keys(o), keys);
        let s3 = crate::JSValue::from_bits(keys[3]).as_string_ptr();
        store_routed_in::<true>(o, s3, 33.0);
        store_unfixed_routed_in::<true>(o, s3, 34.0);
        assert_eq!(live_keys(o), keys, "an overwrite appends nothing");
        assert_eq!(get(o, keys[3]), Some(34f64.to_bits()));
        let t = MetaHome::table(o);
        let i = find(t, keys[5]).unwrap();
        assert!(store_index_in::<true>(o, i, 55f64.to_bits()));
        assert_eq!(get(o, keys[5]), Some(55f64.to_bits()));
        assert!(!store_index_in::<true>(o, (*t).length, 0));
    });
}

#[test]
fn meta_home_delete_by_bytes_creates_no_table_and_refuses_non_configurable() {
    run(|| unsafe {
        crate::gc::initialize_runtime_fixture();
        let _no_move = crate::gc::GcSuppressScope::new();
        let o = meta_home_new() as usize;
        let a = atom("mh_dl_a");
        assert!(delete_in::<true>(o, b"mh_dl_a"));
        assert!(
            MetaHome::table(o).is_null(),
            "a delete without a Table creates none"
        );
        assert!(delete_in::<true>(o, b"mh_dl_never_interned_q7"));
        let b = atom("mh_dl_b");
        insert_key_bits_in::<true>(o, a, 1f64.to_bits(), 0);
        insert_key_bits_in::<true>(o, b, 2f64.to_bits(), 0);
        assert!(delete_in::<true>(o, b"mh_dl_a"));
        assert_eq!(live_keys(o), vec![b]);
        assert!(delete_in::<true>(o, b"mh_dl_never_interned_q7"));
        insert_key_bits_in::<true>(
            o,
            a,
            3f64.to_bits(),
            crate::object::key_attrs::ENTRY_NON_CONFIGURABLE as u8,
        );
        assert!(
            !delete_in::<true>(o, b"mh_dl_a"),
            "a non-configurable key stays"
        );
        assert_eq!(live_keys(o), vec![b, a]);
    });
}
