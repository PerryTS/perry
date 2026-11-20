fix(gc): two moving-collector soundness hardenings for arrays.

1. `layout_note_slot` now restores `SIDE_MASK` state whenever it records a pointer into an array's existing element mask. Previously a stale `POINTER_FREE` state could linger with a populated mask (e.g. after an array was truncated to a numeric/empty prefix), and `heap_payload_slot_selection` treats `POINTER_FREE` as "no pointers" — skipping the whole payload without consulting the mask. The evacuating young-gen minor then dropped live within-length pointer elements, later read as a garbage pointer (`TypeError: value is not a function`). Same class as #6831.

2. `js_array_alloc` / `js_array_grow` now HOLE-initialize the `[length, capacity)` slack instead of leaving it as raw arena bytes. Uninitialized slack could hold stale pointer-shaped bits that any beyond-`length` scan/trace would follow into freed/relocated memory; HOLE is a non-pointer sentinel (matching `js_array_alloc_with_length`).

Verified with `PERRY_GC_FROMSPACE_SCAN`: within-length stale array→young edges drop 202→0; uninitialized-slack false-positive edges drop from ~4000/cycle to ~0. `cargo test -p perry-runtime` array/layout suites green.
