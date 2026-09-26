//! #11319: an exiting thread releases its entries in perry-runtime's
//! process-global closure side tables.
//!
//! Those tables outlive the thread that inserted an entry, while the young log
//! naming the entry is thread-local and the owner's arena block goes back to
//! the allocator at thread exit. A left-behind entry therefore read, once
//! another thread's arena reused the address, as that thread's young owner
//! its log never noted — the `gc/young_log.rs` rule-2 panic that aborted this
//! suite — and it handed its props to any closure later allocated there.
//!
//! Only reachable with perry-runtime built as an ordinary dependency: its own
//! unit tests keep freed blocks mapped and make these tables per-thread.

extern "C" fn probe_thunk(_closure: *const perry_runtime::ClosureHeader) -> f64 {
    0.0
}

const PROP: &str = "__perry_11319_thread_exit_probe";

#[test]
fn thread_exit_releases_the_threads_closure_side_table_entries() {
    let (owner, set_while_alive) = std::thread::spawn(|| {
        use perry_runtime::closure as c;
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let closure = scope.root_raw_mut_ptr(c::js_closure_alloc(probe_thunk as *const u8, 0));
        let proto = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(0));
        // Re-read through the handles: the allocation above may have moved the
        // closure, and the side tables follow a moved owner to its new key.
        let owner = || closure.get_raw_mut_ptr::<perry_runtime::ClosureHeader>() as usize;
        c::closure_set_dynamic_prop(owner(), PROP, 7.0);
        c::closure_mark_key_deleted(owner(), PROP);
        let proto_bits = perry_runtime::JSValue::pointer(
            proto.get_raw_mut_ptr::<perry_runtime::ArrayHeader>() as *const u8,
        )
        .bits();
        c::closure_set_static_prototype(owner(), proto_bits);
        // The subject must be live before the thread exits, or the absence
        // asserted below proves nothing.
        let set_while_alive = c::closure_has_own_dynamic_prop(owner(), PROP)
            && c::closure_is_key_deleted(owner(), PROP)
            && c::closure_static_prototype(owner()).is_some();
        (owner(), set_while_alive)
    })
    .join()
    .unwrap();

    assert!(
        set_while_alive,
        "the entries must exist while their thread lives"
    );
    assert!(
        !perry_runtime::closure::closure_has_own_dynamic_prop(owner, PROP),
        "a dead thread's closure props outlived its heap"
    );
    assert!(
        !perry_runtime::closure::closure_is_key_deleted(owner, PROP),
        "a dead thread's deleted-key entry outlived its heap"
    );
    assert!(
        perry_runtime::closure::closure_static_prototype(owner).is_none(),
        "a dead thread's static-prototype entry outlived its heap"
    );
}
