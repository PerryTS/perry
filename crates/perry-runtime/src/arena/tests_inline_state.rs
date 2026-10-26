//! ThreadHeap proof is established before generated code, including workers.
use super::*;

#[test]
fn inline_state_thread_heap_proof_rejects_an_uninitialized_thread() {
    tests::run_with_fresh_arenas(|| unsafe {
        let raw = block::inline_state_hot_addr().cast::<InlineArenaState>();
        assert!(
            !inline::prove_inline_arena_state(raw),
            "negative control must start uninitialized"
        );
        let generic = js_inline_arena_state();
        assert_eq!(generic, raw);
        assert!(inline::prove_inline_arena_state(raw));
    });
}

#[test]
fn gc_init_establishes_inline_state_on_each_live_worker_heap() {
    let rendezvous = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let rendezvous = rendezvous.clone();
            std::thread::spawn(move || unsafe {
                crate::agent::enter_worker_agent();
                crate::gc::gc_init();
                let raw = block::inline_state_hot_addr().cast::<InlineArenaState>();
                assert!(inline::prove_inline_arena_state(raw));
                assert_eq!(raw, js_inline_arena_state());
                let identity = (
                    raw as usize,
                    (*raw).birth_flags as usize,
                    (*raw).birth_seeds as usize,
                );
                rendezvous.wait();
                let size = crate::gc::GC_HEADER_SIZE
                    + std::mem::size_of::<crate::array::ArrayHeader>()
                    + 3 * std::mem::size_of::<u64>();
                let allocated = js_inline_arena_slow_alloc(raw, size, 8);
                assert!(!allocated.is_null());
                // GC_STORE_AUDIT(POINTER_FREE): type, flags and size contain no heap references.
                allocated
                    .cast::<crate::gc::GcHeader>()
                    .write(crate::gc::GcHeader {
                        obj_type: crate::gc::GC_TYPE_ARRAY,
                        gc_flags: crate::gc::GC_FLAG_ARENA,
                        _reserved: 0,
                        size: size as u32,
                    });
                let array = allocated
                    .add(crate::gc::GC_HEADER_SIZE)
                    .cast::<crate::array::ArrayHeader>();
                array.write(crate::array::ArrayHeader {
                    length: 0,
                    capacity: 3,
                    named: std::ptr::null_mut(),
                    ek_prefix: 0,
                    ek_kind: 0,
                });
                for i in 0..3 {
                    // GC_STORE_AUDIT(POINTER_FREE): TAG_HOLE is an immediate value.
                    crate::array::array_elements_ptr(array)
                        .cast::<u64>()
                        .add(i)
                        .write(crate::value::TAG_HOLE);
                }
                assert_eq!(
                    raw,
                    js_inline_arena_state(),
                    "refill must preserve the TLS address"
                );
                identity
            })
        })
        .collect();
    let mut identities = threads.into_iter().map(|t| t.join().unwrap());
    let a = identities.next().unwrap();
    let b = identities.next().unwrap();
    assert_ne!(a.0, b.0);
    assert_ne!(a.1, b.1);
    assert_ne!(a.2, b.2);
}
