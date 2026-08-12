# `rung1` agent — #6759 C3 ladder, **rung 1**: make the shape word uniform

Scope, per brief: rung 1 ONLY. Not rung 2 (eager birth stamping in codegen),
not rung 3 (switching the nine keys-token consumers), not rung 4 (#7916's
header shrink). Branch `gc/6759-rung1-uniform-shape-word`, worktree
`/Users/amlug/projects/perry/wt-rung1`, target `$HOME/cargo-targets/rung1`.

Reads: `CLAUDE.md`, `gc-handoff/SHAPE-NOTES.md` §§1–5,8, `docs/shape-tree-plan.md`,
#6759, #7916.

---

## 0. What "uniform" turned out to mean concretely

**One sentence:** the shape WORD became uniform; the observation TOKEN did not,
and could not, because they have different consumers.

`ObjectHeader.parent_class_id` already *was* the shape word — the stamp was
just additionally gated on `class_id == 0` at nine sites. Rung 1 deletes that
conjunct and routes every site through three new accessors in
`object/shapes.rs`, so the rule is now literally one predicate:

```
the word is a ShapeId  <=>  is_shape_id(word)
```

★ **The emitted IR already spelled it that way.** All three PICs
(`property_get/generic_dispatch.rs:536-546`, `expr/proxy_reflect.rs:536-546`
and `:862-871`) derive their receiver token as

```
is_stamp = (parent_class_id - 0x8000_0000) <u 0x4000_0000
token    = is_stamp ? (zext parent_class_id | 1<<62) : keys_array
```

with **no `class_id` load at all**. So rung 1 is not "introduce a new mode" —
it is "make the runtime agree with the IR". Before rung 1 the two disagreed
harmlessly only because nothing ever put a ShapeId in a class instance's word.

### 0a. The split that made rung 1 smaller than the plan assumed

SHAPE-NOTES §5's rung-1 sketch lists `typed_feedback.rs:770-806` among the
gates to relax, and predicts the observable as "`typed_feedback::object_shape()`
starts returning **id tokens** for class instances, which makes their PIC tokens
survive GC moves".

**That conflates two different token populations, and doing it breaks the
build.** Measured, not argued — see §3.

| population | producer | compared against | rung-1 verdict |
|---|---|---|---|
| **PIC tokens** | `field_get_set/ic_miss.rs::js_object_get_field_ic_miss` | the *same site's* previously primed token, and the IR's own re-derivation | **switch to ids** — required, else a stamped receiver never hits |
| **observation / guard tokens** | `typed_feedback.rs::object_shape()` | a **codegen-supplied keys POINTER** (`@perry_class_keys_C`), passed in as `expected_keys` | **keep the keys address** — an id can never equal that pointer |

`object_shape()` does NOT feed `generic_dispatch`'s PIC. It feeds
`typed_feedback` observation and the guard family in `typed_feedback/guards.rs`,
whose contracts require `shape_addr == expected_keys as usize`
(`method_direct_call_contract:131`, and the class-field / element-shape
contracts at `:268` and `:523`). Returning an id there fails **every** such
guard **closed** — memory-safe, but it silently deletes the direct-method-call
route and the class-field fast paths, i.e. exactly the tier this ladder exists
to make cheaper.

So: **rung 1 changes the header word, not the observation token.** Migrating
the nine consumers off keys pointers is rung 3, which is what SHAPE-NOTES §3c
already says. `object_shape()` keeps its `class_id == 0` gate with a comment
naming the two tests that go red if anyone drops it.

---

## 1. The change

`crates/perry-runtime/src/object/shapes.rs` (+~95 LOC incl. the block comment):

```rust
shape_word_is_writable(obj) -> bool        // refuses a RegExpHeader alias
object_shape_stamp(obj)     -> u32         // 0 = unstamped
stamp_object_shape(obj, keys, key_count) -> u32   // mint + write, 0 = refused
clear_object_shape_stamp(obj) -> bool      // clears iff the word holds a stamp
```

Nine sites routed through them, all by deleting a `class_id == 0` conjunct:

| # | site | role |
|---|---|---|
| 1 | `object/mod.rs::set_object_keys_array` | CLEAR on keys-pointer change |
| 2 | `object/delete_rest.rs:392` | CLEAR on in-place compaction |
| 3 | `field_get_set/ic_miss.rs:713` | MINT at PIC-miss resolution |
| 4 | `field_get_set/ic_miss.rs:762` | token KIND primed into the PIC cache |
| 5 | `field_get_set/get_field_by_name_tail.rs:1516` | FIELD_CACHE key (read) |
| 6 | `field_get_set/get_field_by_name_tail.rs:1636` | MINT + FIELD_CACHE key (store) |
| 7 | `field_set_by_name/tail.rs:553` | birth stamp on the null→first-key edge |
| 8–9 | `proxy/put_value.rs:396,504,629` | **already uniform** — no change needed |

Sites 8–9 are worth stating: the dynamic/static write-PIC token derivations
never had a `class_id` gate. They were already spelling the rung-1 rule, which
is independent evidence the rule is the right one.

### 1a. Free correctness rider

Two of the four mint sites (`get_field_by_name_tail`, `field_set_by_name/tail`)
had **no RegExp-alias check**; the other two did. A `RegExpHeader` aliases
`GC_TYPE_OBJECT` with a different layout — offset 4 is the high half of
`regex_ptr` (reads as `class_id == 0` on every 48-bit-address target, so the old
gate never excluded it) and **offset 8 is the low half of `pattern_ptr`**.
Routing all four through `stamp_object_shape` closes that in the same edit.

### 1b. What did NOT change

- `class_field_inline_guard` still compares `keys_array`. It loads offsets
  0/4/12/16 and **never offset 8**, so rung 1 cannot affect it. Switching it is
  rung 3.
- Codegen is untouched. `lower_call/new_alloc.rs:543` still writes `parent_cid`
  as a constant, so a fresh `new C()` reads as **unstamped** until its first
  by-name resolve. The stamp is LAZY by construction; eager birth stamping is
  rung 2.
- `perry/thread` serialization: rung 0 (#7981) already reads the parent edge
  from the class-id-keyed registry, so a stamped instance serializes its real
  parent. That is the dependency that made rung 1 possible at all.

---

## 2. Cost

`git diff --stat origin/main`: **7 files, +411 / −108** — of which ~150 lines
are new tests and ~90 are the shapes.rs doc block. Production logic delta is
roughly **60 lines net**, well inside the brief's ~150 estimate. Runtime-only,
as costed.

---

## 3. The entry gate tripped, exactly as predicted

`object/delete_rest.rs::delete_leaves_a_class_instance_with_no_shape_word_to_transition`
went red the moment class instances gained a stamp. Its final assertion carried
its own replacement instruction:

> "the header word changed — if this is now a minted ShapeId,
> `class_field_inline_guard` can switch to a one-word compare and **this test
> should be replaced by that assertion** (#6759 C3)"

Replaced by `delete_mints_a_fresh_shape_id_for_a_class_instance`, which is the
class-instance twin of the plain-object test directly above it: stamp present
after the first by-name resolve → `delete` clears it → next resolve mints a
**different** id. The old test's other two assertions (`class_id` preserved,
keys pointer moved) are **kept inside the new test**, because both are still
true and both are still what the guard compares until rung 3.

Two more added beside it:

- `class_siblings_share_one_shape_id_until_one_is_deleted_from` — pristine
  siblings share ONE id (else an id-comparing PIC would be monomorphic per
  OBJECT and never hit), and a delete moves only the deleted-from instance.
- `a_stamped_class_instance_still_resolves_a_three_level_parent_chain` — the
  brief's risk assertion. Stamping OVERWRITES `parent_class_id`; a 3-level
  `js_instanceof` chain must still resolve after the word is clobbered. It
  asserts the pre-state (`parent_class_id == MID`) and that the resolve
  actually stamped, so it cannot pass vacuously.

### 3a. Two other tests went red — and they are the finding

Running the full `perry-runtime` lib suite after the first (naive) pass:

```
typed_feedback::tests::typed_feedback_method_direct_guard_passes_for_exact_registered_method
    left: 0, right: 1                       # guard returned FAIL
typed_feedback::tests::typed_feedback_class_field_get_guard_requires_raw_f64_layout_when_requested
    assertion failed: site.representation_invalidations >= 1
```

Both are downstream of relaxing `object_shape()`. This is what §0a is about:
they are the pinning tests for the keys-pointer contract, and they did their
job. Reverting that one site (keeping the other eight relaxed) makes both green
again with no other change.

★ **Had I only run the new tests, this would have shipped as a silent
performance cliff** — the guards fail closed, so nothing crashes and no output
differs. Worth recording as another instance of "the gate runs but its subject
never did": a `.ts` probe cannot see a guard that merely stopped firing.

### 3b. A third test's population disappeared

`ic_miss.rs::pointer_token_prime_stamps_epoch_and_goes_stale_on_bump` asserted
"class instance must prime the raw keys pointer token" — and named class
instances as *"the population that still primes raw keys pointers"* (plain
objects took the #6804 id token). Rung 1 stamps class instances, so
`js_object_get_field_ic_miss` now mints-then-primes an id for every receiver
whose mint succeeds, and the pointer arm has **no source-constructible
production population left**. It survives as the id-exhaustion fallback
(`alloc_shape_id` returns 0 after 2^30 shape births) and as what the emitted hit
predicate computes for an as-yet-unstamped receiver.

Split into two tests rather than deleted:

- the epoch mechanics (prime snapshots the live epoch; a bump strands it) now
  drive `pic_prime_get` directly, so the `cache[2] == @PERRY_IC_EPOCH` guard is
  still proved able to FAIL;
- `a_class_instance_primes_an_id_token_after_rung1` asserts the rung-1
  behaviour end-to-end, including `assert_ne!(cache[0], keys)` — priming a keys
  pointer for a stamped receiver would be a permanent miss at that site, so this
  is the test that keeps the runtime's choice and the IR's choice the same.

**Consequence worth flagging for rung 3:** the read PIC's epoch guard
(`epoch_ok = is_stamp || epoch_eq`) is now bypassed on essentially every hit,
because essentially every primed token is an id. Id-token soundness therefore
rests entirely on `shapes::prune_dead_shape_keys` dropping a dead keys array's
record before its address is recycled (wired into both
`gc/copying.rs:1962` and `gc/oldgen.rs:1171`, so it runs on every collection).
That was already true for plain objects since #6804; rung 1 extends the
population. For a class instance the keys array is the process-rooted canonical
one, so the added population is on immortal arrays — the mortal case is the
post-delete clone, identical in kind to a plain object's.

---

## 4. Validation

Compiler `$HOME/cargo-targets/rung1/release/perry` (106 MB — the size that
confirms the `-p perry -p perry-runtime-static -p perry-stdlib-static` set;
100 MB would mean the wrappers were dropped and cargo features re-unified),
`PERRY_RUNTIME_DIR` pinned to the same dir, `.a` mtimes verified after the edit.

| gate | result |
|---|---|
| `cargo test --lib -p perry-runtime` (CI's per-PR scope for this diff) | **2250 passed, 0 failed, 4 ignored** |
| 19-app corpus, byte-exact + exit 0 | **19/19** |
| …under `PERRY_GC_PROTECT_FROMSPACE=1 _DEPTH=800` | **19/19** |
| …under `PERRY_GC_VERIFY_EVACUATION=1` | **19/19** |
| `probe_delete_isolate_ka.ts` vs node 26.5.1 | byte-identical |

### 4a. The discriminating shape — and why it is NOT vacuous for rung 1

SHAPE-NOTES §4b's fixture (`≥4 fields, delete the FIRST, read a mid field,
non-numeric declared types`) was built to isolate `ka_ok` for rung 3. It earns
its place here for a **different** reason:

★ **Rung 1 makes a delete-compacted class instance read-PIC-cacheable for the
first time.** Before it, such an instance's keys array is a private clone, so
`keys_cacheable_for_pic` (SHAPE_SHARED only) refused it and every read fell to
the slow path forever. Rung 1 stamps it, so it primes an id token and the
emitted hit path starts serving it. `probe_delete_isolate_ka.ts` is precisely a
program that reads `s.b` / `s.c` off a compacted instance — i.e. the first
program in which that new surface is exercised. It is a real gate here, not a
borrowed one.

The runtime twin is
`ic_miss::a_compacted_class_instance_primes_a_token_a_pristine_sibling_cannot_match`,
which asserts the compacted instance primes a token a pristine sibling cannot
match **and** that the slots really differ (2 vs 1), so it cannot pass by both
being unprimed.

---

## 5. Sabotage-verify — fix committed FIRST

Commits `c0d26ba72` / `63c44b23e` landed before any sabotage, so
`git checkout --` restores the sabotage, never the fix.

### 5a. `ka_ok` sabotage ON TOP of rung 1 — the fixture still discriminates

Dropped `ka_ok` from all three emitters in
`perry-codegen/src/expr/class_field_inline_guard.rs` (the `acc &= ka_ok`
conjunctions and the two subclass-arm halves), rebuilt the compiler only, same
runtime archives.

| arm | `S post b/c` | `S2 after write` | `N post b/c` (control) |
|---|---|---|---|
| node 26.5.1 | `B C` | `B2 C` | `2 3` |
| **rung 1, base** | `B C` | `B2 C` | `2 3` |
| **rung 1, `ka_ok` sabotaged** | **`C D`** | **`B2 D`** | `2 3` |

Exactly SHAPE-NOTES §4b's table, reproduced on top of rung 1. Two things follow:
rung 1 did not change what `ka_ok` uniquely covers (so rung 3's gate is still
the right one), and the numeric twin stays correct on BOTH arms, which is the
control proving the fixture measures `ka_ok` and not something else.

### 5b. Per-site sabotage of rung 1 itself — which test pins which site

Each relaxed site had its pre-rung-1 `class_id == 0` gate restored **one at a
time**, then `cargo test --lib -p perry-runtime`:

| site | catcher(s) |
|---|---|
| A `set_object_keys_array` clear | **none alone** — see below |
| B `delete_rest` clear | **none alone** — see below |
| **A + B together** (= the pre-rung-1 state) | `delete_mints_a_fresh_shape_id_for_a_class_instance` |
| C `ic_miss` mint | `a_compacted_class_instance_primes_a_token_…` |
| D `ic_miss` token kind | `a_class_instance_primes_an_id_token_after_rung1`, `a_compacted_class_instance_primes_a_token_…` |
| E `get_field_by_name_tail` FIELD_CACHE key | **none** — accelerator-only, by construction |
| F `get_field_by_name_tail` mint | `delete_mints_…`, `class_siblings_share_one_shape_id_…`, `a_stamped_class_instance_still_resolves_a_three_level_parent_chain` |
| G `field_set_by_name/tail` first-key stamp | **none** — earliness-only, by construction |

★ **A and B are each other's backup, and the sabotage is how I learned it.**
`js_object_delete_field` **always** allocates a fresh clone
(`js_array_alloc` → `set_object_keys_array`, `delete_rest.rs:302-322`), so the
keys POINTER always changes on a delete and site A fires. Site B exists for an
in-place compaction that no current path produces. SHAPE-NOTES §1b calls B "the
whole of today's shape transition on delete"; that is half right — A does it
first. Both were present (class-gated) before this PR, so the redundancy is
pre-existing, not introduced here. Recorded rather than removed: B is the only
clear that would still fire if a future path compacts without reallocating, and
deleting it would silently make that path wrong.

The append case is why A alone is not load-bearing: SHAPE-NOTES' invariant
allows a same-pointer append to KEEP its stamp (slots are append-only, existing
mappings stay valid), and a *clone*-on-append re-mints an id that describes the
same prefix. The one key-set change a stamp must not survive is the
**compaction**, and that is exactly where A and B overlap.

**E and G are accelerator-only and I am not claiming otherwise.** E swaps the
FIELD_CACHE key from the keys address to the stamp; every hit re-validates the
stored key, so the delta is that entries survive grow-reallocs and GC moves —
faster, never different. G moves a class instance's stamp earlier (to the
null→first-key edge) than the resolve paths would; sabotaging it only delays the
stamp. Neither has a behavioural assertion, and building a hit-counter probe for
them was judged out of proportion for rung 1. Rung 3, which makes the id
load-bearing in a guard, will need E pinned.
