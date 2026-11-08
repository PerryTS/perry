use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

per_test_global! {
    static EVALUATIONS: AtomicUsize = AtomicUsize::new(0);
}
// Two emitted guards: the guard byte, then the record number (`module_init.rs`).
static mut GUARDS: [u64; 2] = [1, 1];

extern "C" fn shared_evaluation_body() {
    EVALUATIONS.fetch_add(1, Ordering::Relaxed);
}

#[test]
fn folded_evaluation_bodies_keep_distinct_module_records() {
    let _lock = crate::gc::global_side_table_test_lock();
    EVALUATIONS.store(0, Ordering::Relaxed);
    // Windows ICF can give two modules the same evaluation body address.
    // Their emitted mutable guard globals still identify separate modules.
    let guards = std::ptr::addr_of_mut!(GUARDS).cast::<u64>();
    unsafe {
        guards.write(1);
        guards.add(1).write(1);
    }
    // Records are named by the numbers assigned on first evaluation.
    let first = MODULE_INIT_SEQUENCE.load(Ordering::Relaxed) + 1;
    let keys: Vec<_> = (0..2)
        .map(|i| format!("\0module-init:{:x}", first + i))
        .collect();
    MODULE_PATH_REGISTRY.with(|r| {
        for key in &keys {
            r.remove_for_test(key);
        }
    });
    unsafe {
        let body = shared_evaluation_body as *const () as usize as i64;
        super::js_run_module_init_guarded(body, guards as i64);
        super::js_run_module_init_guarded(body, guards.add(1) as i64);
        super::js_run_module_init_guarded(body, guards as i64);
    }
    assert_eq!(EVALUATIONS.load(Ordering::Relaxed), 2);
    MODULE_PATH_REGISTRY.with(|r| {
        for key in &keys {
            r.remove_for_test(key);
        }
    });
}
