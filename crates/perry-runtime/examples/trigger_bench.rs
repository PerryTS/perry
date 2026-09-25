//! Instruction-count probe for the allocation-point GC trigger (#10698).
//!
//!     cargo build --release -p perry-runtime --example trigger_bench
//!     valgrind --tool=callgrind target/release/examples/trigger_bench <mode> 90000
//!
//! Subtract `plain` (the loop alone) and divide by the count. `check` calls
//! `gc_check_trigger` directly, `malloc` pays it inside `gc_malloc`, `arena`
//! is a runtime nursery allocation, which reaches the trigger only when a
//! block fills. Keep `malloc` under the 100 000-object malloc trigger: past
//! it the malloc arm is due, and with loop polls on the collection is
//! deferred to a safepoint this loop never reaches, so every later call
//! evaluates the full ladder by design.
use std::hint::black_box;

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "check".into());
    let count: u64 = std::env::args()
        .nth(2)
        .map(|n| n.parse().unwrap())
        .unwrap_or(90_000);
    perry_runtime::gc::js_gc_init();
    let _ = perry_runtime::arena::js_inline_arena_state();
    let mut sum = 0u64;
    for i in 0..count {
        match mode.as_str() {
            "check" => perry_runtime::gc::gc_check_trigger(),
            "malloc" => {
                let p = perry_runtime::gc::gc_malloc(16, perry_runtime::gc::GC_TYPE_STRING);
                sum = sum.wrapping_add(black_box(p) as u64);
            }
            "arena" => {
                let p =
                    perry_runtime::arena::arena_alloc_gc(32, 8, perry_runtime::gc::GC_TYPE_STRING);
                sum = sum.wrapping_add(black_box(p) as u64);
            }
            "plain" => sum = sum.wrapping_add(black_box(i)),
            _ => panic!("expected check, malloc, arena or plain"),
        }
    }
    println!("{sum}");
}
