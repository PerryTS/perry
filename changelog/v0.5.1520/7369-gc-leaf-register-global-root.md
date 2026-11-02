### Changed

**Two runtime helpers admitted to the GC-effect allowlist — and a measured null result on binary size.**

`js_gc_register_global_root` was the single most frequent non-leaf callee in the
probe suite (148 call sites), and it is provably GC-leaf. Its entire body is:

```rust
runtime_write_barrier_root_heap_word(*root);              // shade one header
GLOBAL_ROOTS.with(|r| r.borrow_mut().push(root));         // TLS Vec push
```

The first call is exactly what `js_write_barrier_root_heap_word` — already
`CannotCollect` — wraps in one line. The second is a `Vec::push`, and the
"malloc count threshold" GC trigger does not apply to it: that counter is
`MALLOC_STATE.objects.len()`, a registry of Perry GC objects, and the
`#[global_allocator]` is plain mimalloc/System with no GC hook.
`js_typed_feedback_maybe_dump_trace` joins its already-admitted family siblings
(env read, JSON serialise, file write; empty body without `diagnostics`).

**The result, measured A/B on the same tree, is that this buys nothing:**

| probe | safepoints | roots | total bytes | `__text` |
|---|---:|---:|---:|---:|
| `06_string_retention` | 105 → 100 | 27 → 27 | 0 | −4 B |
| `09_try_catch_roots` | 343 → 339 | 259 → 259 | 0 | −4 B |
| `11_collect_at_depth` | 120 → 117 | 36 → 36 | 0 | −4 B |

Root counts are **identical**. The 40 safepoints removed across the suite were
all rootless, and a safepoint with no live roots costs essentially nothing —
which is precisely what `docs/engine-plan.md` already says ("Statepoints have no
fixed cost… the axis is not 'statepoints are bigger', it is 'roots are bigger'").

This is worth recording as evidence rather than a win: **the safepoint-count
lever is not the binary-size lever.** Sequencing step 2's "reduce root density"
must attack live-root *sets*, not safepoint counts. Anyone reaching for the next
obvious helper should read the second test below first.

Two tests come with it. `register_global_root_tracks_the_barrier_it_wraps` pins
the two classifications together so a future demotion of the barrier cannot
leave its wrapper claiming to be leaf. `allocating_helpers_are_not_cannot_collect`
pins `js_nanbox_string` **out** of the allowlist: at 120 call sites it is the
obvious next candidate and it reads as pure bit manipulation, but its
null-pointer guard calls `js_string_from_bytes` to allocate an empty string
rather than boxing null.

Probe suite: 11/11 byte-identical to the Node oracle under `PERRY_RS4GC=1
PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1`.
