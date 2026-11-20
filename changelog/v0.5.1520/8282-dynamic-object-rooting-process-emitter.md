**GC: two #8220-class rooting fixes, and a native-stack diagnostic.**

`js_dynamic_object_get_property` held the receiver as a raw pointer across
`js_string_from_bytes`, which allocates. It now opens a `RuntimeHandleScope` and
takes the key through `across_const`, so the receiver is re-read after the
allocation rather than naming from-space if a copying minor lands in the window.

Process `EventEmitter` listener closures are held as raw `*const ClosureHeader`
in a TLS `HashMap`, which the precise root map cannot see — a copying minor that
evacuated one left the table pointing at from-space. `PROCESS_EMITTER` now has a
registered mutable root scanner.

Adds `PERRY_GC_SCAN_NATIVE_STACK=1`, a debug-only scan for stale from-space
pointers on the native stack after a copying minor, with
`PERRY_GC_SCAN_NATIVE_STACK_ABORT=1` to stop at the first offender. **Abort
implies scan**, matching `fromspace_scan::resolve_scan_knobs`: the abort switch
alone would otherwise return at the enabled-gate, never run, and report success.

Neither fix resolves the seeded rooting crash — seeds 8, 11 and 22 still fail.
They are landed as correct fixes in their own right, not as a resolution.
