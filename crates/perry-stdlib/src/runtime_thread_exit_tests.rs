//! #11319: an exiting thread releases its entries in perry-runtime's
//! process-global closure side tables.
//!
//! Those tables are gone: a closure's own properties, deleted-key marks and
//! static prototype now live in its own property bag and state record, traced
//! edges of the closure, so they die with the thread's heap. The closure test
//! below therefore proves the three stores happen while the thread lives and
//! does not probe the dead address afterwards: `is_closure_ptr` reads the
//! candidate before proving ownership, and this test binary unmaps a dead
//! thread's blocks.
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

extern "C" fn probe_thunk(
    _closure: *const perry_runtime::ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    0.0
}

const PROP: &str = "__perry_11319_thread_exit_probe";
/// Deleting a key also removes its own value, so the deleted-key entry is
/// recorded under a second key: the live-thread premise needs all three.
const DELETED: &str = "__perry_11319_thread_exit_deleted";

#[test]
fn thread_exit_releases_the_threads_closure_side_table_entries() {
    let (owner, set_while_alive) = std::thread::spawn(|| {
        use perry_runtime::closure as c;
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let closure = scope.root_raw_mut_ptr(c::js_closure_alloc(
            perry_runtime::fn_info!(probe_thunk, 0),
            0,
        ));
        let proto = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(0));
        // Re-read through the handles: the allocation above may have moved the
        // closure, and the side tables follow a moved owner to its new key.
        let owner = || closure.get_raw_mut_ptr::<perry_runtime::ClosureHeader>() as usize;
        c::closure_set_dynamic_prop(owner(), PROP, 7.0);
        c::closure_mark_key_deleted(owner(), DELETED);
        let proto_bits = perry_runtime::JSValue::pointer(
            proto.get_raw_mut_ptr::<perry_runtime::ArrayHeader>() as *const u8,
        )
        .bits();
        c::closure_set_static_prototype(owner(), proto_bits);
        // The subject must be live before the thread exits, or the absence
        // asserted below proves nothing.
        let set_while_alive = c::closure_has_own_dynamic_prop(owner(), PROP)
            && c::closure_is_key_deleted(owner(), DELETED)
            && c::closure_static_prototype(owner()).is_some();
        (owner(), set_while_alive)
    })
    .join()
    .unwrap();

    assert!(
        set_while_alive,
        "the entries must exist while their thread lives"
    );
    assert_ne!(owner, 0, "the probe closure must have been allocated");
}

// #11471: one file per audited group of process-global tables.
mod listeners_tests;
mod reactors_tests;
mod stdlib_misc_tests;
mod streams_tests;
mod symbols_tests;
mod ui_tests;
