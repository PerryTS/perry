### Fixed

- **`js_native_call_method` reused one copy of its rooted receiver across ~1160 lines of allocating probes (#7528).** The function *does* root the receiver — `object_handle` on its first lines — and then read it out exactly once:

  ```rust
  let object = object_handle.get_nanbox_f64();
  let jsval  = JSValue::from_bits(object.to_bits());
  ```

  A value read out of a root and held in a local **is not rooted**: the root keeps the object alive and the collector rewrites the *slot*, not the copy (`docs/src/internals/gc-rooting-invariant.md`). Those two locals were then used across a dozen probes that allocate, so each one received a receiver address a moving collector may already have invalidated.

  The measured deref was the closure-magic probe: `is_closure_ptr(raw_addr)` on an address derived from the stale copy, faulting **5/5** under `PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=200`, with lldb stopping on the magic read itself (`ldr w8, [x28, #0xc]`, `CLOSURE_MAGIC`).

  Not latent: `test_gap_gc_iterator_drain_rooting` printed `badLen 1` instead of `badLen 0` — one of 50,000 clones losing its `tags` array.

  Both are now closures, so every use is a fresh slot read. That is the point: it makes each of the 93 sites correct **by construction** rather than by an audit of which probes allocate — an audit that would have to be redone every time a line is added to a 1,168-line function. The cost is a slot load against a dispatch tower orders of magnitude more expensive, and the function already used this idiom for `refreshed_args`.
