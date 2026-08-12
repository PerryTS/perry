# #7949 — JS values retained in Rust containers across allocating calls

Working notes for the `fix/7949-root-container-values` branch. Written
incrementally; the PR body is the summary, this is the audit trail.

## The class

A runtime helper accumulates NaN-boxed JS values into a plain `Vec` and keeps
filling or walking it across calls that can allocate. The `Vec` lives on the
Rust heap: it is not a shadow slot, not a temp root, and not reachable from any
registered scanner, so an evacuating minor can neither keep those objects alive
nor rewrite their addresses.

Two properties make it the worst class to debug, both of which held here:

* **`scripts/gc_root_dominance_check.py` is structurally blind to it.** It reads
  emitted LLVM IR. A Rust-side container is not in the IR.
* **Nothing faults at the collection.** The nursery recycles the address and the
  stale word reads a valid *unrelated* object. Both end-to-end sabotage runs
  below surfaced as `TypeError: value is not a function` — the canonical
  late-surfacing form — not as a segfault at the offending read.

The reproducibility heuristic in CLAUDE.md held exactly: this is a *container*,
not a register, so it goes bad at collection #0 and stays bad. Both end-to-end
tests fail deterministically on the pristine build, every run.

## Sites fixed

### 1. `crates/perry-runtime/src/object/groupby.rs`

`group_by_collect` returned `Vec<(f64, f64)>` filled across a **user callback**
(`js_closure_call2`) run once per element. Everything already in the vector was
stale from the first collection onward. Three more holes in the same function
family, all of which the issue's summary did not name:

* the materialized input array pointer (`raw`) and the closure pointer
  (`cb_ptr`) were hoisted out of the loop and re-dereferenced on every
  iteration, across the callback;
* `Object.groupBy`'s result object (`obj` / `obj_boxed`) was a bare local across
  one `group_by_make_array` + one `js_string_from_bytes` per group — and it is
  reachable from nothing else, so it had **no root at all**;
* `group_by_make_array`'s freshly allocated array was returned across
  `rebuild_array_layout_from_slots`, and the caller's `arr_boxed` then spanned
  the key-string interning.
* `Map.groupBy` held the result `Map` and every key across per-group array
  allocation and `js_map_set`.

Also: `Object.groupBy` coalesced Symbol keys through a
`HashMap<u64 /* symbol address */, usize>`. A fresh `Symbol(desc)` is a GC
allocation, so a symbol that moved mid-loop hashed to a new bucket and started a
**duplicate group**. Now keyed on `SymbolHeader::id`, a monotonic `u64` that an
evacuation copies verbatim — the same reasoning #7246 used to intern symbol
descriptions by id.

### 2. `crates/perry-runtime/src/object/object_ops/define_properties.rs`

`keys: Vec<f64>` collected the descriptor bag's own names, then walked them
across, per key: `str_from_value` (a `js_string_coerce`, which allocates for
every shape except an already-heap string), `js_dynamic_object_get_property`
(runs a **user getter** on the properties bag), and `js_object_define_property`
(grows the target). The receiver, the properties bag, the own-names array and
the coerced key string were all bare locals across the same window.

### 3. `crates/perry-runtime/src/proxy/own_keys.rs` (sweep, item 5)

* `alloc_key_array(&[f64])` is the shared builder for `Object.keys` /
  `getOwnPropertyNames` / `getOwnPropertySymbols` / `Reflect.ownKeys` on a
  Proxy. It pushed each key into a growing array — and growing allocates — so
  every key after the first collection point named from-space.
* `proxy_enum_own_keys` held the trap's key list and the accepted-so-far list
  across `js_reflect_get_own_property_descriptor`, i.e. across the proxy's
  `getOwnPropertyDescriptor` trap: arbitrary user JS, once per key.

## The reusable rule

`crates/perry-runtime/src/gc/roots/rooted_values.rs` adds `gc::RootedValues`: a
growable list whose elements are `RuntimeHandle`s. The handle stack is a
registered *mutable* root scanner (`MutableRootScannerSource::RuntimeHandles`),
so elements are marked **and** their slots rewritten. `get(i)` re-reads the slot,
so the correct shape is shorter than the incorrect one.

It is not a proof — Rust has no effect system for "this call may allocate", and
`get` still hands back a bare `f64`. What it removes is the *container* as the
hole.

**Documented hazard (module docs):** never push to an outer `RootedValues` while
an inner `RuntimeHandleScope` is alive. `RuntimeHandleScope::drop` truncates the
stack to its base, which would silently discard the outer container's newest
handles. Every helper touched here uses exactly one scope.

`RootedValues` holds no `static`/`thread_local!`, so
`scripts/gc_runtime_root_holders.py` requires no verdict for it — it borrows the
existing, already-registered handle stack rather than adding a new root holder.
No new `get_raw_{mut,const}_ptr` sites either, so
`scripts/raw_handle_debt.py`'s ratchet is unmoved.

## How the fix is proven

`crates/perry-runtime/src/gc/tests/rooted_container_values.rs`, four tests, all
under `CopyingNurseryTestGuard` + a forced `collect_minor_trace`:

1. `rooted_values_elements_survive_a_collection_that_moved_them` — asserts
   `copied_objects > 0` **and** that every element's address changed **and**
   that the post-collection pointer still reads the original bytes. Address
   change is the "actually moved it" half; the byte check is the "still the same
   object" half.
2. `plain_vec_of_values_is_not_a_root` — the sabotage arm. The identical
   workload in a bare `Vec<f64>` keeps naming pre-collection addresses while a
   rooted witness in the same cycle moves. This is what makes (1) non-vacuous:
   if the instrument could not tell rooted from unrooted, this test would fail.
3. / 4. `object_group_by_…` / `map_group_by_…` — end to end through the real
   `#[no_mangle]` entry points with a callback that forces an evacuating minor
   on **every** element, then asserts each group's contents by string bytes.

### Sabotage verification (fix committed first)

With `crates/perry-runtime/src/object/groupby.rs` reverted to `origin/main` and
everything else left in place:

```
test gc::tests::rooted_container_values::map_group_by_items_survive_a_moving_minor_in_the_callback
  ... TypeError: value is not a function
test gc::tests::rooted_container_values::object_group_by_items_survive_a_moving_minor_in_the_callback
  ... TypeError: value is not a function
```

Both abort the harness process; with the fix restored all four pass. The
sabotage arm therefore proves the tests exercise the fixed code path, and the
failure mode is the exact late-surfacing shape the class is known for.

### Compiled probe

`test-files/test_gap_gc_container_value_rooting.ts` drives all three helpers
from compiled TypeScript, with `churn()` inside every callback so a back-edge
poll is emitted in user JS *and* the retired from-space bytes are recycled after
it. Run under `PERRY_GC_SCHEDULE_RATE=1`.

## Sweep (issue item 5): the rest of the population

A scan of `perry-runtime/src` for functions that (a) declare a `Vec<f64>` /
`Vec<(f64…)>` / `Vec<JSValue>` accumulator, (b) push inside a loop, (c) call
something that can allocate or run user JS, and (d) have no
`RuntimeHandleScope`, returns 14 functions. Triage:

| site | verdict |
|---|---|
| `proxy/own_keys.rs` `alloc_key_array`, `proxy_enum_own_keys` | **FIXED here** — user traps per key |
| `typedarray/iterate.rs:52` `js_typed_array_filter` | **REAL, not fixed.** `kept: Vec<f64>` spans a user callback per element (BigInt64/BigUint64 elements are `BIGINT_TAG` heap pointers), and `ta`/`recv` are hoisted raw locals across the same callback. The `ta` half is #6949 shape (a), not the container shape; both want one change. Follow-up. |
| `closure/dispatch/value_call.rs` (`js_native_call_value`, `js_closure_call_array`), `closure/dispatch/bound.rs` `dispatch_bound_function`, `object/class_constructors.rs` ×5, `object/class_registry/parent_static.rs` ×2, `proxy/apply_construct.rs` `forward_apply` | argument-gathering: the `Vec` is filled from an already-materialized source with no allocating call *between* pushes, then handed to one call. Not the accumulate-across-a-callback shape. Left alone deliberately — converting them would cost a handle push per argument on the hottest dispatch paths for no demonstrated window. |
| `array/splice_slice.rs` `js_array_splice` | removed-element buffer; the loop that fills it does not call user code. |

`object/descriptors.rs`'s `js_object_get_own_property_descriptors` — the one the
issue quotes as carrying the "builder helpers follow this convention" comment —
was already converted by #6943/#7341 and needs nothing.

The broader "cold arms never root anything" population the #6949 scope note
describes is still open and is not what this branch claims to close.

## #7803

See the PR body / final report.
