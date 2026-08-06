### Fixed

**`Promise.all` at scale rejected with a resolution value, or with
`TypeError: value is not a function` — two stale from-space reads, neither of
them in the promise machinery's logic (#7497).**

The app-pattern kernel `promise_all_chains` printed `Uncaught (in promise) 0`
under `PERRY_NO_AUTO_OPTIMIZE=1` and a rooting-shaped `TypeError` under the
default link, and was the last blocker on the public benchmark artifact.
`crates/perry-runtime/src/promise/`'s settle/reject/microtask logic is unchanged
by this PR; both defects are the #7341 family — *a value read out of a root and
held in a register across a call that allocates is not rooted*.

**1. The `globalThis` builtin lookup (the one the reproducer proves).**
`js_get_global_this_builtin_value` — the canonical `globalThis.<Builtin>` read
behind `instance.constructor`, bare `Date`/`Array`/`Object` identifier
resolution, and `is_default_promise_constructor` — read the global object out of
its root into a raw `*const ObjectHeader` and only THEN allocated the lookup key:

```rust
let global_obj = js_nanbox_get_pointer(js_get_global_this());
let key = js_string_from_bytes(name);            // ALLOCATES -- may collect
js_object_get_field_by_name(global_obj, key);    // from-space deref
```

The root is not the problem — `THREAD_GLOBAL_THIS` is registered and evacuation
rewrites it. The ORDER is: this lookup interns nothing, so every call mints a
fresh string, and any of those allocations can be the copying minor that moves
`globalThis`.

`Promise.all` is the shape that finds it. Each element runs
`Call(promiseResolve, C, «next»)`, i.e. `Promise.resolve`, and
`js_promise_resolve_spec` asks `is_default_promise_constructor` for
`globalThis.Promise` through that helper — so one `Promise.all` over 50 000
already-resolved promises performs 50 000 of these lookups inside a single native
call, and one of them straddles the collection.

**2. The spec combinators' own locals.** With (1) fixed,
`PERRY_GC_PROTECT_FROMSPACE=1` moved the fault one frame out, to `run_combinator`
itself. `perform` ran its per-element loop — `Call(promiseResolve, C, «next»)`
and `Invoke(nextPromise, "then", …)`, both of which run USER JS — while holding
the `elements` snapshot, the shared `values` and remaining-count arrays, the
capability's `resolve`/`reject` and the constructor in bare Rust locals.
`build_element_closure` was worse than stale-read: it took all five of its GC
arguments in registers, allocated the closure, and *stored the pre-collection
addresses into the capture slots* — publishing from-space into an object the
collector then maintains. `new_promise_capability`, the two `Promise.allSettled`
element functions, `build_settled_{fulfilled,rejected}`,
`make_resolving_functions` (which stores the promise AND the shared
already-resolved guard into two closures after four allocations) and
`combinator_iterable_to_array`'s array fast path (which carried the array being
cloned across `own_symbol_property`, a call that can run a user getter) all had
the same shape.

Everything is rooted in a `RuntimeHandleScope` and every address is re-read at
its point of use. The per-element handles live in a scope INSIDE the loop, so a
50 000-element combinator does not push 50 000 entries onto the handle stack.
All handles are NaN-boxed rather than `root_raw_*_ptr`, so
`scripts/raw_handle_debt.py` is unchanged at 999.

**Three more callers of `js_get_global_this()` had (1)'s shape** and are fixed
the same way. These come from auditing the callers, not from a reproducer, and
are called out as such: `class_meta.rs`'s builtin-constructor name walk (worse
than the proven site — a fresh key allocation inside a ~50-iteration loop against
one address read before the loop), `js_globalthis_seed_async_local_storage`
(`globalThis` is the RECEIVER of a store that follows two allocations), and the
`Temporal.<Type>.prototype` walk (three hops, each with a key allocation between
the receiver read and its dereference). The four sites of this shape in
`error.rs` / `with_env.rs` were already fixed by #6943; these are the ones that
sweep missed.

**Why it read as "a separate promise-rejection defect".** A stale read returns
whatever from-space happens to hold, so `globalThis.Promise` came back as a
non-callable — or, when the garbage was zero, as the resolution value `0`
arriving on the rejection path. The two link modes printed different messages for
the same defect. It was untouched by #7495 only because #7495 fixed a different
function.

**Localisation, for the next person** (each knob against the unfixed binary):
`PERRY_GEN_GC=0` and `PERRY_WRITE_BARRIERS=0` both make it pass while
`PERRY_GC_MOVING_SAFEPOINT=0` does not — the first two make the copying minor
ineligible, the third only disables the *safepoint* collection, and the one that
matters here is the alloc-point direct minor (`trigger=ArenaBytes
declared_safepoint=false`; there is exactly ONE collection in the whole run).
`PERRY_GC_FROMSPACE_SCAN=1` reported `clean`: no HEAP slot held a stale pointer,
which is the signature of a holder in a native frame rather than in a table.

### Added

**`test-files/test_gap_gc_global_builtin_lookup_rooting.ts`**, registered in
`test-parity/gc_repsel_corpus.txt` so `gc-moving-witnesses` runs it. One wide
`Promise.all` (50 000 elements) rather than the kernel's 1000 × 50: the single
wide call packs enough lookups between two collections to fail on the shipped
default in a fraction of a second, where the kernel needs ~20× the work for the
same window. Deterministic — 6/6 runs before the fix.

### Changed

`scripts/auto_opt_app_patterns.sh` no longer skips `promise_all_chains`; the skip
list is empty and the gate covers all 12 kernels. Its rot check means the line
had to come out with the fix. Every array expansion is now guarded on
`${#…[@]}`: macOS ships bash 3.2, where `set -u` turns `"${EMPTY[@]}"` into an
"unbound variable" abort, so an empty skip list would have stopped the gate
before its first kernel — CLAUDE.md hazard 4 wearing a different hat.
`--self-test` still passes.
