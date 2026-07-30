### Fixed

- **`Array.prototype` hole reads no longer hang after the prototype relocates (#6981).** Reading an index that was never assigned, on a program that has installed an indexed property on `Array.prototype`, could drive the runtime into unbounded recursion and die with `SIGSEGV` on the thread's stack guard page ("Thread stack size exceeded due to excessive recursion"). It was not memory corruption — every dereference on the way down was to valid, mapped memory.

  `array::indexing` memoizes `Array.prototype`'s (and `Object.prototype`'s) heap address in a process-global `AtomicUsize`. That is a raw pointer to a **movable** object, and nothing maintained it. Every reader of an array pointer resolves it through `clean_arr_ptr`, which follows `GC_FLAG_FORWARDED` chains; the cache did not. The hole/out-of-bounds read fallback guards against self-recursion with the object-identity test `proto != receiver`, so once the prototype moved, the two sides named the same object by two different addresses, the guard stopped firing, and `js_array_get_f64` ⇄ `array_oob_prototype_get` called each other forever.

  Two independent relocations reach it, and only one is the collector:

  - **`js_array_grow`** — an indexed write past the dense capacity (`Array.prototype[300] = v`) reallocates the backing store and forwards the old head. **No GC is involved**: this reproduces with `PERRY_GEN_GC=0`, in the shipped configuration, with no environment overrides at all.
  - **the copying young-gen minor** — it evacuates the prototype and forwards. This is the form #6981 measured, reachable in the shipped collector under a heap budget.

  Fixed with three defences: `array_prototype_addr` / `object_prototype_addr` resolve the forwarding chain and heal the cache in place (covering `js_array_grow`, which the collector never sees, with a call-free not-forwarded fast path); `scan_prototype_addr_cache_roots_mut` is registered in `gc_init` so a relocating cycle rewrites the slot like every other address-holding side table (needed because a swept, recycled from-space stub no longer carries the forwarded bit); and `array_oob_prototype_get` resolves the receiver before the identity compare, making the guard exact by construction.

  Same root cause also closes a silent-miss class: before this, a *first* `Array.prototype` index write occurring after a relocation would not set the pollution flag at all, so holes read `undefined` instead of the inherited value.

  `scripts/gc_repsel_matrix.sh --arms all --pressure 8` goes from `PASS=325 UNVER=100 XFAIL=1 FAIL=14` to `PASS=339 UNVER=100 XFAIL=1 FAIL=0` over 440 cells (byte-exact vs Node 26.5.0: 425/440 → 439/440), with nothing regressed. All 14 failures were `test_gap_repsel_p4a3_numarray_barriers`, in exactly the 14 arms whose `[gc-copy-minor]` count is non-zero. `evac_minor` and `force_verify` are back in the per-PR arm subset, so "a representation regressed GC correctness under relocation" is now a per-PR signal.
