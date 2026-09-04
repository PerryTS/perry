### Performance

- **Shape descriptors: an id-indexed slab and two compact reverse indices
  instead of a boxed hash map and two `Vec`-valued ones; owned growth history
  is retired** (#9706). `PERRY_GC_CENSUS` on the compiled
  claude-code TUI at idle put the agent-local shape table at ~330 bytes per
  live descriptor. The bytes were the STORAGE, not the facts: a 56-byte
  `Box<ShapeDescriptor>` in a 64-byte allocator bin, a 16-byte
  `PtrHashMap<u32, Box<_>>` entry sitting at ~25% load after
  `shrink_to(2 * len)`, a 57-byte bucket in the exact-facts reverse map plus a
  16-byte `Vec<u32>` buffer per entry, and a 33-byte bucket in the keys-address
  reverse map — four allocations and three hash tables saying the same thing.

  `crates/perry-runtime/src/object/shapes_store.rs` (new):

  - **`ShapeSlab`** — the by-id store. A ShapeId is `SHAPE_ID_BASE + n` from a
    process-global monotonic counter, so `n` indexes a chunked slab directly:
    no hash, no per-record allocation, and a record address that never moves
    for the record's lifetime, which is the property the collector relies on
    when it enumerates the record's `keys` word as a rewritable slot (#8112)
    and retains that address across budgeted resumptions. Chunks (32
    records, behind a two-level page directory) are allocated lazily — a
    worker's ids interleave with the main thread's, and the claude-code TUI
    mints a million ids at startup for 44 k survivors — and an all-dead chunk
    or page is released at the same cadence as the reverse-index shrink
    (`shrink_shape_tables`, once per major collection).
    The direct-mapped lookup-way cache that fronted the hash map is gone: a
    slab probe IS "shift, index, deref", and it needs no invalidation epoch.
  - **`ShapeRecord`** — the packed 32-byte `#[repr(C)]` table record (`keys`
    first, so the record address is the keys slot). `ShapeDescriptor` stays
    the by-value copy the rest of the runtime consumes, lifted from the
    record; the copy's `record` field is the slab address.
  - **`IdList`** — a 16-byte id list (three inline, a spilled `Vec` beyond)
    that is the value of both remaining reverse indices: `by_facts`, keyed by
    a 64-bit FNV fold of the six identity facts (every hit re-validates the
    record, so a collision costs a second record read, never a wrong answer),
    and `families`, keyed by keys address. The `ShapeFacts`-keyed map,
    `indexed_keys` (the address a record was last indexed under) and the
    per-scan `PROBE_MEMO` map are gone: the family index is the memo, and
    `scan_shape_table_rekey_mut` now probes each keys address ONCE per
    family — with the marking visit when any member is an old/cache
    carrier, which is exactly the #8112 rooting duty.

  A family is small by construction. A SHARED keys array is immutable
  (mutation forks a private clone), so its descriptors differ only in the
  birth bound, a semantic generation, the class kind, or a tombstone count.
  An OWNED array grows in place, and until now every same-address append left
  the predecessor descriptor alive until the array itself died
  (`retain_key_count_versions`): a dictionary built by ten thousand appends
  kept ten thousand prefix descriptors. **`publish_object_shape_from` now
  retires that history behind the version its single owner carries**
  (`retire_owned_shape_siblings`), after the successor is stamped and armed
  (#9200's order), keeping any version an optimization cache permanently owns
  (`cache_carrier`). Sound because `GC_FLAG_SHAPE_SHARED` is sticky: an
  unflagged array has had exactly one owner for its whole life, a stale IC
  token already misses on the stamp compare, and `shape_descriptor_by_id` of
  a retired id is `None`. The one owner whose history IS reinstalled — an
  Array-subclass receiver, whose tail-transition cache learns the
  (predecessor, successor) pair right after the publish and stamps the
  predecessor back on `pop` — keeps the old behaviour, gated on the receiver
  class the learner itself is scoped to.

  `PERRY_GC_CENSUS` rows: `shapes.ids_by_facts` / `shapes.ids_by_keys` are
  replaced by `shapes.by_facts` / `shapes.families`; `shapes.descriptors` now reports the slab's
  real bytes; new `shapes.ids_minted(process)`,
  `shapes.descriptors.carried(live objects)` / `.uncarried` (with the
  `cache_carrier` / `old_carrier` split) and `shapes.families.multi` /
  `.largest` rows say how the population relates to the live heap.

  `scripts/shape_descriptor_census.py` pins the new invariants (slab chunks
  individually boxed and never reallocated; `keys` first in the record;
  owned-history retirement scoped to the family, keeping cache carriers, and
  ordered after the stamp) with sabotage self-tests for each.

Measured on the compiled claude-code TUI (`cli_2.1.112.js`,
  `PERRY_GC_CENSUS` via `SIGUSR2` at idle, the same compiled objects relinked
  against the two runtimes, third census = after `shrink_shape_tables`):

  | | before | after |
  |---|---|---|
  | live descriptors | 68,661 | 43,724 |
  | `shapes.descriptors` | 9.40 MB | 3.82 MB |
  | facts reverse index | 8.39 MB | 1.64 MB |
  | keys-address reverse index | 2.67 MB | 0.89 MB |
  | shape tables total | 22.22 MB (29.86 at the first census) | 8.11 MB (8.45) |
  | RSS at census | 441 MB | 419 MB |

  Of the remaining 43.7 k descriptors 8,664 are carried by a live object;
  the rest are per-object semantic generations and shared-array prefix
  versions whose keys array is still alive, which nothing prunes yet.

  Benchmarks (min of 5): `bench_dynamic_property_keys` 38/12 → 37/12 ms,
  `bench_populated_delete` 62 → 58 ms, `bench_shared_shape_delete` 45 →
  40 ms; a 150,000-key dictionary built by appends 11.7 s → 0.23 s (on
  `main`, `retain_key_count_versions` rebuilt the whole same-address id
  list on every append), 2,000 × 200-key dictionaries 491 → 246 ms, and
  200 k `{a,b,c}` literals with 20 hot read passes 103 → 47 ms.

  Validation: `cargo test -p perry-runtime` 3113 passed (dev and release,
  single-threaded); `scripts/run_lint_gates.sh` 64/64; a 233-test
  shape/object/class/GC/JSON gap subset gives identical verdicts on both
  arms (224 PASS, 2 pre-existing PARITY_FAIL).
