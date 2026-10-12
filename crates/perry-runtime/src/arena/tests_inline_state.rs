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
                let allocated = js_inline_arena_slow_alloc(raw, 40, 8);
                assert!(!allocated.is_null());
                allocated
                    .cast::<crate::gc::GcHeader>()
                    .write(crate::gc::GcHeader {
                        obj_type: crate::gc::GC_TYPE_ARRAY,
                        gc_flags: crate::gc::GC_FLAG_ARENA,
                        _reserved: 0,
                        size: 40,
                    });
                allocated.add(8).cast::<crate::array::ArrayHeader>().write(
                    crate::array::ArrayHeader {
                        length: 0,
                        capacity: 3,
                    },
                );
                for i in 0..3 {
                    allocated
                        .add(16 + i * 8)
                        .cast::<u64>()
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
