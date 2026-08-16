# Shape tree (#6759 Phase C) — design and implementation record

Status: IMPLEMENTED IN STAGES; audited against `main` on 2026-08-13.

Phase A (`RuntimeState`, per-thread hot tables) and Phase B
(`ObjectHeader.meta` + `GC_TYPE_OBJECT_META`) landed before the first Phase C
change. This document began as the Phase C review gate; the audit below records
where the shipped implementation deliberately differs from that original end
state.

## Implementation audit

| area | landed | deliberately still separate |
|---|---|---|
| Phase A: explicit runtime state | #6795 grouped the descriptor, object-storage, field-lookup, shape, and transition hot tables. The exotic-expando table is now in the same `RuntimeState`. Later work made remaining runtime TLS use the `perry_thread_local!` hot cache and added the Darwin TLS budget gate (#7758). | Receiver-specific registries that are not on the ordinary-object hot path remain in their owning modules. Phase A did not turn every runtime TLS value into a `RuntimeState` field. |
| Phase B: self-describing headers | #6796 added the traced `ObjectHeader.meta` edge and migrated custom prototypes. #6800 added per-owner descriptor summary words. Object-owned overflow storage later moved into `ObjectMeta.spill`. | Property/accessor descriptor payloads are still authoritative in address-keyed tables. Date, RegExp, Error, Promise, Map, Set, and Temporal use distinct GC cell layouts, so their kind and expando payloads cannot be represented by `ObjectHeader.meta` without first unifying those headers. Their tables retain GC rekey/prune/clear-on-allocation defenses. |
| Phase C: first-class shapes | #6797 added shared key→slot `Shape` records; #6801/#6803 added stable, never-reused ShapeIds and GC rekeying; #6807/#6808 made allocation and read PICs compare discriminated shape tokens. #7981/#7983/#8009/#8010 then made the header shape word uniform across plain objects and class instances and birth-stamped every known allocator. | `keys_array` remains the ordered key artifact. The transition cache and `FIELD_CACHE` still exist as shape-keyed accelerators, and churn-heavy objects do not switch to an authoritative dictionary representation. `Object.keys` still creates a fresh result by walking the keys array, as required by the JS API. |

The architectural construction is therefore in production: a per-thread
runtime state, a traced per-object metadata edge, stable shape identity, and
exact shape-token PIC guards. The stronger literal reading of the original RFC — no
address-keyed descriptor payloads, uniform exotic headers, shape-resident
transition edges, and formal dictionary mode — is not implemented and should
not be inferred from the merged phase labels.

The original “within 1.5× Node” table is historical. Maintainer direction on
Issue `#6759` raised the performance bar to beat Node and split the remaining compiler
coverage into follow-up campaigns. #6811 beat Node on the canonical object-write
micro; #6812 tracks generalizing that narrow win. Static inline `in` caches and
shape-cached enumeration remain separately scopeable work rather than hidden
requirements of the already-landed shape identity.

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
- **C3 — stable shape identity, staged** *(refined at implementation
  time; design review on #6798)*:
  - **C3a (landed)**: `Shape` records carry a stable `shape_id`; an
    owned keys array's grow-realloc MIGRATES the record instead of
    orphaning it, and GC evacuation rekeys records to the moved array
    (metadata-rewrite scanner). Kills the O(key_count) re-index per
    capacity doubling in the wide regime.
  - **C3c-r (landed, runtime-only)**: ids come from a PROCESS-GLOBAL
    allocator in a dedicated u32 range (`0x8000_0000..0xC000_0000`) —
    disjoint from every real/builtin class id, and globally unique so a
    worker-serialized stamp can never alias another thread's ids. Plain
    objects (`class_id == 0`) carry their id in the otherwise-dead
    `parent_class_id` header word, stamped lazily at read resolution and
    cleared whenever the keys pointer changes or a delete compacts
    in-place (ids are never reused, so stale entries can only miss).
    `FIELD_CACHE` keys on the stamp — entries survive grow-reallocs AND
    GC moves. The PIC's miss handler now primes only `SHAPE_SHARED`
    (process-rooted, address-immortal) keys arrays, closing a latent ABA
    hazard where an owned array's recycled address could satisfy the
    unvalidated inline compare.
  - **C3-codegen (landed, then generalized)**: #6807/#6808 added eager id
    stamping and discriminated ShapeId PIC tokens. #7981 moved the serialized
    inheritance edge to the class registry; #7983 made the header word a
    uniform shape word for plain objects and class instances; #8009/#8010
    birth-stamped the compiled and runtime allocator families so one shape's
    population cannot split between pointer and id tokens. Folding the
    transition cache into shape-resident edges remains deferred.
- **C4 — dictionary mode: largely subsumed.**
  The concrete goals — per-shape hash lookup for wide objects, churn
  not corrupting acceleration, eager invalidation on delete/compaction,
  recycling safety — are delivered by C1 (shared per-shape slot maps),
  C3a (identity survives grow), and C3c-r (never-reused ids; compaction
  drops record + stamp). The remaining formalization (an authoritative
  per-object dictionary carrying enumeration order, off the transition
  system) is deferred until profiling shows transition-cache pollution
  from churn-heavy objects; keys_array remains the order artifact.
- **C5 — typed_feedback exactness, staged.**
  - **C5a (landed)**: the class-field inline-guard disable is vetted
    per-key against declared instance-field names (with a late-class
    retro-check), so babel-style prototype method installs stop
    poisoning `this.field` access process-wide.
  - Remaining guard families may now vet one exact shape id; eager stamping is
    no longer their blocker.

C1, C2, C3a, C3c-r, and C5a were runtime-only. The C3 codegen work and later
uniform-shape-word follow-ups landed under their own reviews. New performance
claims should use the current follow-up issue's measurement protocol rather
than the original #6759 absolute timings.

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

1. **Class instances — resolved for identity.** C1 deliberately did not touch
   them. #7981/#7983/#8009/#8010 made their header shape word and birth-stamp
   discipline match plain objects without disturbing class-registry vtable or
   inheritance dispatch. Class layouts still remain their field-definition
   source; “uniform” means the cache identity contract, not identical storage.
2. **Delete/compaction** rewrites keys_arrays in place for owned arrays —
   C1 handles it via `indexed_len` shrink detection (same as
   WIDE_KEY_INDEX today); C4 is the real answer.
3. **Enumeration order**: keys_array insertion order is the spec order
   source today; shapes must never reorder it (C1-C3 keep keys_array as
   the order artifact; C4 moves order into the dictionary).
4. **Per-thread shapes** mean workers rebuild shape tables — same as all
   Phase A state; acceptable (workers rebuild every cache today).
