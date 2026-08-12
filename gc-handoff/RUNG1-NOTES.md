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
(`property_get/generic_dispatch.rs:479-491`, `expr/proxy_reflect.rs:536-546`
and `:862-872`) derive their receiver token as

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
contracts at `:271` and `:525`). Returning an id there fails **every** such
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

`git diff --stat origin/main`, code only (the 7 `crates/perry-runtime/src`
files): **+494 / −106**. Of that, ~225 lines are new tests, ~95 is the
shapes.rs doc block, and the rest is comment. **Production logic delta is
roughly 60 lines net** — well inside the brief's ~150 estimate, and runtime-only
exactly as costed. No public API changed (every new item is `pub(crate)`; no
`#[no_mangle]`/`pub extern` signature moved), so no dependent crate can break on
it.

Rung 1 did **not** turn out to be larger than costed, so there was no reason to
stop and hand back.

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

### 5c. ★ Near-miss worth recording: I nearly gated on a sabotaged binary

After §5a I ran `git checkout --` on `class_field_inline_guard.rs` and moved
straight on to launching the gap suite with `PERRY_SKIP_BUILD=1`. **The source
was restored; the release binary was not.** `$HOME/cargo-targets/rung1/release/perry`
was still the `ka_ok`-sabotaged compiler, and the gap run was 54/554 tests into
grading my change against it before I noticed. Killed and rebuilt.

This is CLAUDE.md's "Verifying a runtime change" trap in a shape the section
does not name: not a stale ARCHIVE from the wrong `cargo build` line, but a
stale BINARY from a sabotage I had already reverted in source. `git status`
was clean, which is precisely the tell that means nothing. The corpus numbers
in §4 were taken *before* the sabotage and are unaffected; the gap run was
restarted against a freshly built clean compiler.

Rule for the next agent: **a sabotage cycle ends with a rebuild, not with
`git checkout --`.** Treat the restore and the rebuild as one step.

**E and G are accelerator-only and I am not claiming otherwise.** E swaps the
FIELD_CACHE key from the keys address to the stamp; every hit re-validates the
stored key, so the delta is that entries survive grow-reallocs and GC moves —
faster, never different. G moves a class instance's stamp earlier (to the
null→first-key edge) than the resolve paths would; sabotaging it only delays the
stamp. Neither has a behavioural assertion, and building a hit-counter probe for
them was judged out of proportion for rung 1. Rung 3, which makes the id
load-bearing in a guard, will need E pinned.

---

## 6. Handoff — what rung 2 and rung 3 inherit

1. ★ **Rung 3's first job is `object_shape()`, not the guard.** SHAPE-NOTES §3c
   lists nine consumers of the keys token; the one that *blocks* is
   `typed_feedback::object_shape()`, because the guard contracts compare its
   return against a codegen keys pointer. Until those contracts take an id,
   `object_shape()` cannot return one, and rung 1 leaves that gate in place with
   a comment naming the two tests. Do that migration and the guard switch
   together, or the intermediate state is a silent fast-path deletion.
2. **Rung 2's stamp must reach the same word rung 1 clears.** Eager birth
   stamping writes `@perry_class_shape_C | (field_count << 32)` at
   `new_alloc.rs:543`. `set_object_keys_array` and `delete_rest` already clear
   it for class instances after this PR, so rung 2 is purely "arrive stamped"
   — no new invalidation is needed.
3. **The read PIC's epoch guard is now nearly bypassed** (§3b). If rung 3 makes
   ids load-bearing in a *guard* (not just a cache), the trust chain is
   `shapes::prune_dead_shape_keys` + `scan_shape_table_rekey_mut`, not the
   epoch. Worth an explicit test that a recycled keys-array address cannot
   inherit a dead record's id.
4. **Per-thread shape tables, process-global ids, process-global PIC caches.**
   Two threads adopting the same `CLASS_KEYS_BY_ID` array mint *different* ids
   for it (each thread's table allocates from the shared monotonic counter), so
   a `@perry_ic_N` global can thrash between two ids for one shape. Not a
   correctness bug (ids are globally unique, so no false hit) and pre-existing
   for plain objects since #6804 — but rung 3 should measure it before relying
   on id-token hit rates.
5. **SHAPE-NOTES §8's four loose ends are all still open**, including the static
   write PIC's missing epoch check — which matters *less* after rung 1 (that PIC
   now mostly sees id tokens) but is still an unintended asymmetry.
6. Sites E and G have no behavioural pin (§5b). Rung 3 needs E pinned.

---

## 7. Gap-suite triage (`scripts/run_gap_tests.sh`, 532/554 = 96.0 %)

The snapshot ratchet reported 8 regressions, 10 status changes and 1 improvement.
None of it may be attributed without an A/B, so:

### 7a. Six of the eight are #7932 — signature-matched, not assumed

| test | exit | panic site |
|---|--:|---|
| `test_gap_fetch_request_from_node_incoming_message` | 134 | `perry-ext-http/src/server/server.rs:911` |
| `test_gap_http_client_no_redirect_follow` | 134 | `perry-ext-http/src/server/server.rs:911` |
| `test_gap_http_overloads_3226plus` | 134 | `perry-ext-http/src/server/server.rs:911` |
| `test_gap_http_req_async_iterator` | 134 | `perry-ext-http/src/server/server.rs:911` |
| `test_gap_http_res_socket_writable_onfinished` | 134 | `perry-ext-http/src/server/server.rs:911` |
| `test_gap_net_connect_bound_value` | 134 | `tokio-1.53.1/.../net/tcp/listener.rs:304` |

All six: *"there is no reactor running, must be called from the context of a
Tokio 1.x runtime"*, zero stdout before aborting. This reproduces #7932's table
**exactly**, including the one detail that discriminates it from a generic
"http tests crash" — five share `server.rs:911` while `net_connect_bound_value`
aborts one frame lower inside tokio's own `TcpListener`. #7932 was A/B'd against
two compilers when filed (3 runs each, `exit=134` 6/6) and closed as a duplicate
of #7629 (release-profile ext-staticlib link defect). It is deliberately **not**
in `gap_snapshot.json`, which is why the gate reads it as `pass -> crash` and why
`run_gap_tests.sh` is red on `main`.

Retired: pre-existing, tracked, not rung 1.

### 7b. ★ A stale-archive trap I walked into inside my own target dir

`libperry_ext_http.a` (20:06) and `libperry_ext_zlib.a` (20:07) in
`$HOME/cargo-targets/rung1/release` were built during my FIRST gap run — the one
against the `ka_ok`-sabotaged compiler (§5c) — and the second run **reused them**
rather than rebuilding, because they already existed. CI evicts exactly these
before its parity job (`rm -rf target/perry-auto-* target/debug/libperry_ext_*.a`,
#5892); an ad-hoc `PERRY_SKIP_BUILD=1` run does not.

Every "regression" and every status change maps to one of the ten ext archives
present in that directory (http, net, zlib, cron, dayjs, moment, ratelimit,
slugify, exponential_backoff, events), which is what made the mapping visible.
Re-running the two parity failures with `PERRY_RUNTIME_DIR` pointed at a clean
directory (no ext archives, `PERRY_NO_AUTO_OPTIMIZE=1`) still diverges, so those
two are NOT stale-archive artifacts — but the general lesson stands and is worth
adding to the runbook: **a reused target dir carries ext staticlibs across arms,
and they are invisible to `git status` just like the runtime `.a`.**

### 7c. Ten `node_fail -> parity_fail` are oracle-coverage, not Perry

A fresh worktree has no `node_modules`, a known phantom-regression source for
this harness, so I symlinked the main checkout's. That changes which tests the
NODE oracle can run, which is exactly the shape of a `node_fail -> parity_fail`
transition. Both arms share the symlink, so the A/B controls for it. (Running
those tests' node command by hand from the worktree root still gives
`ERR_MODULE_NOT_FOUND`, so the harness resolves modules from a different CWD —
either way it is oracle coverage, not Perry behaviour.)

### 7d. Two remain under A/B

`test_gap_specabi_reassign` (`plain: 0 0 2` vs node `99 101 2`) and
`test_gap_zlib_3285_params` (zlib argument validation: `no-throw` vs
`ERR_OUT_OF_RANGE`/`ERR_INVALID_ARG_TYPE`). Neither is in `gap_snapshot.json`,
which — per #7932 — neither convicts nor exonerates, since the snapshot demonstrably
does not enumerate every currently-red test. Both reproduce deterministically.

Prior: neither touches an object-shape surface. `zlib_3285_params` is argument
validation; `specabi_reassign` is a typed-array binding reassigned via `as any`,
whose subject is the #6906/#7052 spec-ABI invalidation in codegen — no classes,
no `delete`, no keys array. But a prior is not evidence, so the control arm
(all seven rung-1 `class_id` gates restored, same tree otherwise — a tighter
control than a pristine `main` build, and it costs no extra disk) is being built
and both tests re-run on it. Verdict recorded below.
