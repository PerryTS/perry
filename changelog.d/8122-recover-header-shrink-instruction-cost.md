### perf(gc, codegen): recover the instruction cost of the 56 B → 48 B header shrink — smaller objects now cost fewer instructions too

#8122 (`ObjectHeader` 56 → 48 B) held on the standing directive — minimize RSS
**and** keep best compute, both — because it bought its footprint win with
+2…+9% instructions on 18 of 22 corpus rows. This measures where every one of
those instructions went and removes them; the shrunk representation now runs
faster than `main` on the rows that regressed most, with the whole footprint
win intact and several rows' peak footprint lower again.

Numbers are `/usr/bin/time -l` instructions retired and peak memory footprint,
best-of-3, both arms built from their own tree with the same `-p perry
-p perry-runtime-static -p perry-stdlib-static`, `PERRY_RUNTIME_DIR` and
`PERRY_CACHE_DIR` pinned per arm, all 19 corpus stdouts byte-compared and
exit-checked. `A` = `main@bfb0707be`, `B` = this branch (#8122 rebased + this).
"#8122 as held" is the same PR before this recovery, on the same host.

| row | #8122 as held | now: instructions | now: peak footprint |
|---|---:|---:|---:|
| `deeplist` | +9.0% | **−17.2%** | **−17.7%** |
| `retain1` | +8.6% | **−11.6%** | −5.6% |
| `retain` | +3.7% | **−6.8%** | −9.7% |
| `shapes` | +3.1% | −1.7% | −7.2% |
| `retain_wide` / `retain_wide1` | +2.3% / +2.0% | −0.6% / −0.3% | −5.5% / −6.0% |
| `interp` / `iso_miss` / `pipeline` | +3.3% / +2.8% / +0.4% | +0.0% / +0.0% / −0.3% | −6.8% / −7.3% / −9.9% |
| `tree` / `tree_wide` | +0.6% / +0.5% | −0.2% / +0.1% | −12.9% / −6.4% |
| `cycles` | +0.8% | −0.3% | **−29.6%** |
| `push_cls` / `churn_alloc` / `churn` | +5.5% / +5.5% / +3.2% | +0.3% / +0.2% / +0.2% | −9.5% / −9.5% / −9.6% |
| `push_num` / `churn_read` / `fib40` | ~0 | +0.1% / −0.0% / −0.1% | ~0 |
| `asyncpipe` | +0.4% | +0.5% | +2.9% (see below) |

The churn family's +0.2–0.3% and `asyncpipe`'s +0.5% are inside those rows'
run-to-run spread (0.2–0.9%); the rest of the column is outside it.

#### What the instructions actually were — four mechanisms, none of them "the probe"

The cost had been attributed to shape-table probes replacing the deleted
`field_count` word. Per-row measurement (GC traces, `sample` on symbolised
arms, `otool` diffs) says otherwise:

1. **`deeplist` / `retain1` / `retain` — the FIRST copying minor was
   byte-denominated.** With no GC at all the mutator is byte-identical between
   arms (`deeplist` at 250k objects: 151.7 M vs 151.5 M). Both arms run exactly
   two minors; but the first fires when Eden holds 16 MB, before any object
   census exists (`MEAN_SURVIVING_OBJECT_BYTES` seeded at the 72 B reference),
   so 48 B objects put **371k** objects into that cycle where 56 B put 318k —
   and the first cycle is the one that must TRACE (no survival estimate yet).
   A traced in-place-promotion cycle cost **~1,600 instructions per object**
   because it resolved the receiver's `ShapeDescriptor` **five times per
   object** (`gc_field_slot_range`, `gc_keys_array_slot`, the slot visitor,
   `object_keys_array_ptr` and `with_shape_shared_descriptor`'s bound check),
   plus a `hot_shape_layouts` probe. 53k extra objects × that price is the
   whole +100 M.
2. **`push_cls` / `churn_alloc` / `churn` — an LLVM store-merging artefact,
   +4.5 instructions per `new`.** IR identical modulo offsets (the shrunk arm
   even had one store fewer); the machine code was not. Before, the two header
   words were both constants and LLVM merged them into one 16-byte
   constant-pool store; after, the second word is `class_id | ShapeId << 32`
   with the ShapeId from a global, so nothing merges and the 40-bit `gc_packed`
   immediate is rematerialised (`mov` + two `movk`) at every allocation.
3. **`interp` / `iso_miss` — a consumer that landed after the PR.** #8094's
   `param_type_guard::plain_object` reads both deleted words; the rebase turned
   two free `u32` loads into `object_is_regular` + `object_live_slot_count`
   (two probes plus a re-read of the already-validated GcHeader), and its
   per-field `js_object_get_field` reads probed once more each.
4. **`pipeline` (+3.9% between two builds of the SAME hot path)** — LTO folded
   `shape_install_shared`+`record` into `init_typed_shape_layout` in one build
   and not the other, making the per-construction memo-hit path an
   811-instruction function whose prologue and spills were paid on every hit.

#### The changes

* **`gc/tenuring.rs`: allocation census before the first minor.** Halfway to
  the base cap (8 MB of from-space), once per process, hop the young
  generation's headers (`arena::young_allocation_census`, ~1 M instructions)
  and seed the object denomination with the ALLOCATED mean, so the first cycle
  buys the same object budget every later one does. The collector's survivor
  census overwrites it at the first minor; the one-sided clamp still applies.
  Side effect: the first minor fires earlier on small-object workloads, which
  is where the extra footprint wins above come from (`cycles` −29.6%,
  `pipeline` −10%, the churn family −9.5%).
* **`gc/promote_in_place.rs`: `UNTRACED_PROMOTION_SURVIVAL_PERMILLE` 990 → 980.**
  The 992 that 990 was read off came from a first cycle at the raw 16 MB band;
  object-denominated, `retain`/`retain1`'s first cycle reads **988** (the same
  ~131 KB of abandoned `all.push` backing stores over a smaller nursery), and
  at 990 their second cycle traced again — `retain1` +13%. Its own exposure
  bound becomes 2.56 MB against the same 32 MB cap; the untraced-bytes budget
  stays the binding bound. Doc fact, `check_gc_doc_claims.py` and the two
  threshold-shaped tests updated to the constant.
* **`gc/layout.rs`, `gc/layout_slot_visit.rs`, `object/gc_slots.rs`: one
  descriptor lookup per traced object.** `gc_child_slots` resolves the
  receiver's `ShapeDescriptor` once and threads it through the field range,
  the keys slot (`gc_keys_array_slot` / `gc_field_slot_range` now take it), the
  shared pointer-mask selection (`HeapChildSlotIterator::new_object`,
  `heap_payload_slot_selection_from`, `with_shape_shared_descriptor_from`) and
  the slot visitor. `with_shape_shared_descriptor` itself drops from two probes
  to one for every field store that reaches it; `object_keys_array_ptr` is
  gone.
* **`gc/layout.rs`: `init_typed_shape_layout` split** — the memo-miss install
  tail is `install_typed_shape_layout_slow`, `#[cold] #[inline(never)]`, so
  the per-construction hit path keeps its shape whatever LTO decides
  elsewhere in the crate.
* **`lower_call/new_alloc.rs`, `codegen/mod.rs`, `codegen/string_pool.rs`,
  `function.rs`: the header image.** The 16-byte prefix `[gc_packed |
  class_id | ShapeId << 32]` is composed ONCE at module init — beside the
  ShapeId mint, into a per-class `<2 x i64>` global — from
  `target_layout::inline_alloc_gc_packed`, the single definition of the packed
  word the site also uses (`inline_alloc_total_size_bytes` alongside it). The
  inline allocator entry-hoists that global like the keys global and stores it
  with one vector store; the site cross-checks the table's packed word and
  class id against its own derivation and falls back to a per-function
  compose (`LlFunction::entry_init_object_header_image`) if they differ.
  Per-function was tried first and fixed loops but not recursion (`tree`
  allocates once per call: +0.6%), hence module init.
  `layout_declared_at_allocation` / `layout_pointer_free_at_allocation` gain
  `_in` forms over the module-level maps so the site and the table run the
  same predicate.
* **`param_type_guard.rs`: one probe per guarded object**, and
  `own_data_field` reads inline slots against the bound `plain_object` already
  resolved (`object/field_get_set/accessors.rs::object_field_at_with_live`,
  which is now also `js_object_get_field`'s body).
* **`object/field_get_set/ic_miss.rs`, `get_field_by_name_tail.rs`,
  `typed_feedback/guards.rs`: one probe per call** on the property-get IC-miss
  path (was three: regularity, descriptor, `object_shape_id` for the token),
  the by-name slow scan (was two, one into an unused binding, plus one inside
  every field read it returned through) and `js_method_direct_shape_class`
  (was two). Same discipline as the collector: resolve once, derive.

#### Tests

* `gc::tests::copying::adaptive_tenuring::allocation_census_seeds_the_first_cap_before_any_minor`
  drives a real nursery past half the base cap, asserts the seed equals THIS
  population's header-walk mean (independently recomputed) and differs from
  the 72 B seed, that the effective cap moved before any collection, and that
  the walk is one-shot; `gc::tenuring::tests::allocation_census_seed_is_gated_and_one_shot`
  pins the gate (below half-cap: nothing; a survivor census disarms it; reset
  re-arms).
* `lower_call::alloc_hot_tests::the_inline_allocator_stores_its_header_prefix_as_one_vector_image`:
  exactly one compose, in module init after the mint; the site loads the
  image global and stores the vector; the allocating function does not compose
  its own.
* `perry-runtime --lib` 2482/0/4; `perry-codegen` VALIDATION_CODEGEN;
  `check_gc_doc_claims.py` and every other `lint` python gate OK; the GC
  canaries (`retain`/`tree`/`churn`/`shapes`/`deeplist`/`retain1`/`push_cls`/
  `interp` × forced evacuation + evacuation verifier, from-space protect
  depth 32, and both together) byte-exact with the instruments demonstrably
  live (`copied_objects` 237k per cycle on `retain` under forced evacuation;
  87 `[gc-fromspace-protect]` retire lines on `churn`).

#### Not closed here

* `asyncpipe` peak footprint +2.8% at 120 batches (+7.7% at 1200). The GC
  arena is identical between arms (same triggers, same 6767 copies, 23 MB
  reserved); the footprint is ~100 MB of **non-arena** memory at 1200 batches
  — the async activation-box retention, growing at the same rate per MB
  allocated in both arms — and mimalloc's own peak (`MIMALLOC_SHOW_STATS`) and
  `maximum resident set size` are both LOWER for the shrunk arm (140.3 vs
  140.9 MiB; 147.0 vs 149.7 MB) while `peak memory footprint` is higher. The
  two OS metrics disagree in direction, i.e. this is about how much
  freed-but-resident memory is marked reusable at the peak instant, not more
  live data. Left as measured.
