use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

per_test_global! {
    static EVALUATIONS: AtomicUsize = AtomicUsize::new(0);
}
static mut GUARDS: [u8; 2] = [1, 1];

extern "C" fn shared_evaluation_body() {
    EVALUATIONS.fetch_add(1, Ordering::Relaxed);
}

#[test]
fn folded_evaluation_bodies_keep_distinct_module_records() {
    let _lock = crate::gc::global_side_table_test_lock();
    EVALUATIONS.store(0, Ordering::Relaxed);
    // Windows ICF can give two modules the same evaluation body address.
    // Their emitted mutable guard globals still identify separate modules.
    let guards = std::ptr::addr_of_mut!(GUARDS).cast::<u8>();
    let keys: Vec<_> = (0..2)
        .map(|i| format!("\0module-init:{:x}", unsafe { guards.add(i) } as usize))
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
