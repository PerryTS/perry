# Shape tree (#6759 Phase C) — design

Status: DRAFT for design review (the Phase C entry gate #6759 calls for).
Prerequisites: Phase A (`RuntimeState`, per-thread hot tables) and Phase B
(`ObjectHeader.meta` + `GC_TYPE_OBJECT_META`) — both stacked under this
plan's first landing.

## Goal

Property access at fixed offsets, scanning eliminated, ICs exact — the V8
object model. Acceptance (from #6759): read/write/define/`in`/`Object.keys`
all within ~1.5× node across the micro suite; babel-class module init ≤2×
node.

## The insight this plan builds on

Perry already HAS a degenerate shape system — it just isn't first-class:

- A **shared `keys_array` is a shape.** Object literals born from the same
  codegen site share one keys_array (`GC_FLAG_SHAPE_SHARED`, the shape
  cache); `js_object_set_field_by_name` clones-before-mutate, so a given
  keys_array instance is immutable-in-practice once shared. Two objects
  with the same keys_array pointer have, by construction, the same keys at
  the same slots.
- The **transition cache is the shape-transition edge set.** Its entries
  are exactly `(prev_keys, key_ptr) → (next_keys, slot_idx)` — V8's
  transition tree, keyed by keys_array identity, already cached.
- The **codegen literal `shape_id`** (packed-keys hash → prebuilt
  keys_array via the shape cache) is the "root shapes" table — but it is
  a transient cache key, discarded at allocation; nothing per-object
  stores it. The keys_array pointer is what survives, and it is already
  what `typed_feedback::object_shape()` returns, what the read prop-plan
  and the transition cache key on.
- What's MISSING is the per-shape payload V8 hangs off a Map: the
  key→slot descriptor table (Perry rebuilds per-OBJECT key indexes
  instead — `KEYS_INDEX` keyed by object address, `WIDE_KEY_INDEX` keyed
  by keys_array but capped at a 4-entry LRU that thrashes past 4 wide
  shapes), attributes, and an exact identity an IC can compare in one
  load.

Today FOUR encodings of "this object's shape" coexist and never share a
table or an invalidation signal: the transient literal/class shape_id,
the keys_array pointer (read plan, wide index, transition cache,
typed_feedback), `class_id` (store plan, class-field guards, vtable), and
the anon-shape class-id set (`.constructor === Object` only). Class
instances allocated without a keys_array (`class_id != 0`,
`keys_array == null`) are a fourth representation with no array at all —
they are C3's unification problem, untouched before then.

So Phase C is not "bolt a foreign object model on" — it is promoting the
existing keys_array-identity system into an explicit, queryable `Shape`
record, then letting each consumer (reads, defines, `in`, enumeration,
typed_feedback guards, prop plans) switch from scanning/re-deriving to
asking the shape.

## Shape record (C1 form)

Per-thread, in `state().shapes` (Phase A gives us the home):

```rust
struct Shape {
    /// Identity: the shared keys_array this shape describes.
    keys_id: usize,
    /// Key count at index-build time (staleness check: a keys_array is
    /// append-only while shared; longer = extend incrementally,
    /// shorter = drop (delete/compaction happened)).
    indexed_len: u32,
    /// FNV-1a content hash of key bytes → slot(s). Content-validated on
    /// every hit against the actual stored key (the KEYS_INDEX /
    /// WIDE_KEY_INDEX trust model, which also makes address reuse safe:
    /// a recycled keys_array address fails validation and the entry is
    /// dropped and rebuilt).
    slots: HashMap<u64, SmallVec<u32>>,
}
```

Keyed on `keys_id` (the keys_array address) in a `PtrHashMap`. No new GC
hooks in C1: the trust model is validation-on-hit exactly like the two
tables it replaces (stale entries are inert; a dead keys_array's entry is
dropped on first mismatching probe, and a `clear`-style prune can ride the
existing keys-array sweep hook later if profiling wants it).

Why this is a real shape and not "another cache": it is keyed on SHAPE
identity, not object identity. Today, 10k objects sharing one literal
shape build 10k private `KEYS_INDEX` entries (one HashMap each, built
O(N) per object). Under C1 they share ONE `Shape` whose slot map is built
once. `WIDE_KEY_INDEX` (already keys-keyed but capacity-4 LRU, so any
working set over 4 wide shapes thrashes) folds into the same record,
unbounded.

## Migration ladder

Each step lands independently behind green suites, per the #6759 method.

- **C1 — shapes as first-class records (this PR).**
  `state().shapes`: the `Shape` record above. `keys_index_lookup`,
  `keys_index_insert`, and `wide_key_index_lookup`/`note_hit` re-route to
  it; the per-object `KEYS_INDEX` table and the `WIDE_KEY_INDEX` LRU are
  deleted (along with `clear_keys_index_for_ptr`'s GC sweep hook, replaced
  by a keys-liveness prune in the dead-owner fan-out).
  No header change, no codegen change, no semantic change.
  Cost note: `KEYS_INDEX` was object-keyed precisely so the index survives
  the clone-on-first-insert and grow-reallocs of a building object.
  Keys-keyed shapes instead rebuild once per pointer change — once at the
  shared→owned fork and once per capacity doubling (O(log N) growths),
  amortized O(N) total — in exchange for same-shape SHARING on the
  read/locate side (10k literal-born siblings: one shape build instead of
  10k private index builds) and an unbounded wide-shape working set. Each
  consumer keeps its existing build threshold (write ≥32, read ≥257), but
  a read may consult an entry the write path already built — a strict
  superset of today's acceleration.
  Explicitly NOT subsumed in C1: the read prop-plan (already O(1)
  direct-mapped; folds into shape identity in C3), the store prop-plan
  (class-keyed; needs the shape to carry prototype facts first), and the
  transition cache (already the edge set; becomes shape-resident in C3).
- **C2 — per-key descriptor facts move into the per-object `meta` record**
  *(design refined at implementation time — see below)*. The read/write
  fast paths answer "can an own descriptor cover this key" from two
  Bloom words in the Phase B `meta` record
  (`ObjectMeta::{attr,accessor}_key_bits`, bit `fnv(key) & 63` per
  installed key, monotonic) instead of the per-object descriptor probes
  Phase A grouped: a clear bit — or a still-null meta — is an
  authoritative miss, so `get_property_attrs` / `get_accessor_descriptor`
  return `None` with no `String` build and no table probe, and the
  owner-level form gates the O(table) owner scans (`Object.keys` fast
  path). The address-keyed tables remain authoritative; non-meta-capable
  owners (handle ids, typed arrays, RegExp) stay on the conservative
  probe-always arm.
  **Why not the sketched per-shape attrs sidecar:** shape records are
  keyed on the keys_array ADDRESS with validation-on-hit — a trust model
  that works only for POSITIVE facts (a hit re-validates against array
  content). "No descriptors on this shape" is a NEGATIVE fact with
  nothing to validate against, and the keys identity churns on every
  grow-realloc, so a shape-resident claim goes stale undetectably. The
  `meta` record travels with the object (GC-traced, moved+rewritten with
  its owner, null on every fresh allocation), which is exactly the
  carrier a negative per-object fact needs — and it means descriptor
  install needs NO shape-split (no O(N) keys clone on freeze). True
  shape-resident attributes return in C3, where the shape becomes a
  header-resident identity and descriptor install becomes an explicit
  transition (the V8 model).
  Also fixes a latent stale-read: a fresh object at a recycled address
  can no longer be misread as owning a dead tenant's not-yet-pruned
  descriptor entries (its meta is null, so the gated getters miss
  authoritatively).
- **C3 — the shape pointer becomes the object's identity.**
  `ObjectHeader` gains a shape reference unifying `class_id` and the
  literal shape ids (candidate encodings: reuse the `class_id` u32 as a
  shape-table index — `is_anon_shape_class_id` already carves that space —
  or a `meta`-resident pointer for shaped-divergent objects). ICs and
  prop plans compare shape identity in one load; `keys_array` becomes a
  shape-owned enumeration artifact rather than a per-object scanned
  array. This is the step where `FIELD_CACHE` and the transition cache
  fold into shape-resident ICs/edges, and where codegen property plans
  compile against shape ids (touches `perry-codegen` property_get/set
  and the PIC).
- **C4 — dictionary mode formalized.**
  Wide/high-churn objects (today: KEYS_INDEX_THRESHOLD=32 /
  WIDE_KEY_INDEX_MIN_KEYS=257 heuristics, delete/compaction in
  `delete_rest.rs`) get an explicit out-of-shape mode: the object owns a
  private dictionary (its `meta` record), leaves the transition system,
  and enumeration order is preserved by the dictionary itself.
- **C5 — typed_feedback exactness.**
  Guard families that today re-derive facts per call vet a shape id
  instead; representation-change invalidation (`#6759`'s Phase A rider)
  becomes "shape actually changed".

C1 and C2 are runtime-only. C3 is the codegen milestone and gets its own
review before landing. Acceptance is measured at each step against the
#6759 micro table.

## GC story

- C1: none needed (validation-on-hit trust model; records are per-thread
  plain heap in `RuntimeState`, dropped at thread exit).
- C2: the summary words are POD inside the existing `ObjectMeta` record
  (the trace arm still visits only `prototype`); accessor closures stay
  where Phase A put them (the descriptor tables, whose GC scanner roots
  and rekeys them).
- C3: shape table entries hold `keys_array` references — those become
  GC roots with rewrite-on-move, following the Phase B pattern (the
  shape table is the successor of today's `SHAPE_CACHE_OVERFLOW`, which
  already has exactly those hooks).

## Risks / open questions for review

1. **Class instances** (`class_id != 0`, `keys_array == null`) resolve
   fields via class layout, not keys_array — C1 deliberately does not
   touch them; C3's unification must fold class layouts and anon shapes
   into one shape-id space without disturbing vtable dispatch.
2. **Delete/compaction** rewrites keys_arrays in place for owned arrays —
   C1 handles it via `indexed_len` shrink detection (same as
   WIDE_KEY_INDEX today); C4 is the real answer.
3. **Enumeration order**: keys_array insertion order is the spec order
   source today; shapes must never reorder it (C1-C3 keep keys_array as
   the order artifact; C4 moves order into the dictionary).
4. **Per-thread shapes** mean workers rebuild shape tables — same as all
   Phase A state; acceptable (workers rebuild every cache today).
