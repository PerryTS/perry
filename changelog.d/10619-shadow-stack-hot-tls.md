Give the shadow-stack root state (`SHADOW`) the `tls_hot` fast path. It was the
last savepoint field still declared as a raw `thread_local!`, so every
`CatchSavepoint::capture()` — once per `try` entry — paid `_tlv_get_addr` for
it on Darwin, while `EXCEPTION_STATE`, `CALL_METHOD_DEPTH` and the named
`runtime_handle_stack`/`temp_roots` fields were already routed through
`tls_hot`. Wave 1's "has any thread used this subsystem" latch does not make
the read rare here: it is set by the first shadow-frame push anywhere in the
process, and a `catch (e)` binding needs a shadow slot itself.

Profiling pinned it to one call site, `try_push_with_kind+176 ->
_tlv_get_addr`, about 6% of instructions retired per entry. The change is a
storage-mechanism swap — same type, call sites, const-init and drop-free
semantics — so the address-stability contract `js_shadow_frame_enter` depends
on is unchanged. A non-throwing `try` entry goes 179.3 -> 164.8 instructions
(-8.1%), differenced within each binary with the loop control at the noise
floor in both arms. The probe understates it: `SHADOW` is read on many paths,
not only this one.

Because the shadow stack is the GC's precise root set rather than bookkeeping,
the new fixture attacks rooting: a throw four frames down, a throw crossing
`Array.prototype.map`'s runtime trampoline, `finally` on both paths, nested
`try` with an inner rethrow, a `catch` that itself throws, and 25-level nesting
— every caught value read back after GC-pressure allocation. It plus four
existing exception/rooting fixtures ran under five GC-schedule seeds at
RATE=1 with from-space protection, all byte-identical to node, with
forced_collections=2566 / copying_minors=2566 / moved_objects=37000 confirming
the instrument was live.
