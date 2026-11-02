**perf(runtime): stop hoisting cold-path thread-locals into `o[k]`'s fast lane.**
`js_object_get_field_by_name` resolved two thread-locals unconditionally in its
prologue — the `PROXIES` registry and `RUNTIME_HANDLE_STACK`'s fallback — on a
dynamic-key read loop that never touches a Proxy and never uses a `.size` key.
Both belong to arms that are guarded at the Rust level and never taken.

A `thread_local!` address resolution is `readnone` to LLVM, so once a guarded
cold arm is inlined the optimizer may hoist *just the address computation* above
its guard: the gate survives, the TLS call escapes it. Marking both arms
`#[cold] #[inline(never)]` keeps them opaque to the inliner. The
`runtime_handles` half is the general fix — `RuntimeHandleScope::new()` is called
from dozens of small guarded arms across the runtime, and splitting its fallback
lets the published fast arm stay `#[inline(always)]` without dragging a raw TLS
call to every call site.

Also: `try_data_get_bytes` called `is_plausible_heap_addr` explicitly and again
inside `try_read_gc_header`, which LLVM could not CSE across
`classify_heap_generation`'s intervening cache write; that one site now uses
`try_read_gc_header_known_plausible`.

**544.7 → 512.9 instructions per `o[k]` access (−5.8%)**, differenced within each
binary against a bare-loop control reading 0.00 / −0.10 so layout and fixed
per-process cost cancel. Found by disassembly, not by reading: the profile
charged both `_tlv_get_addr` calls to `js_object_get_field_by_name` itself rather
than to a callee, and `otool -tV` plus `nm` named which two thread-locals they
were. Fixing the Proxy block alone left the other in place by a different route.

Negative results worth not re-running: `try_data_get_bytes`'s `from_utf8` and
Bloom-hash preamble is spec-required work; `is_anon_shape_class_id`'s remaining
11.2% is the per-image `current()` lookup that #10570 already reduced to a hash
plus an 8-slot probe, and a process-global mirror would be unsound across images;
`keys_find_slot_by_bytes`'s `memcmp` is genuine key-byte comparison.

Validation: new `test_gap_dynamic_key_proxy_receiver.ts` covers trapped,
pass-through and nested Proxies plus interleaved plain/Proxy receivers — the arm
made cold must still be correct when taken — byte-identical to node 26.5.1;
#10570's read-paths test unchanged; four GC-stress runs (seeds 1 and 42, from-space
protection, evacuation verification, scan-abort) all exit 0 with `dangling=0`,
`missing_rewrites=0` and non-zero copying minors (32/29/241/247); gap suite 831/838
with all 6 failures pre-existing; `perry-runtime --lib` 4,016 passed with the 2
failures reproduced on pristine `origin/main`.
