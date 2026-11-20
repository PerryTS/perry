### Fixed

- **`scripts/gc_root_dominance_check.py`: the property-GET dispatch is
  `POLL_CAPABLE_RUNTIME`, so `--moving-only` can finally see the GET half of
  property access** (#7154). Two of the effort's instruments disagreed at zod's
  `clone`: `PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`
  faulted deterministically, while `--stale-registers` classified the same
  window `MOVING: no` — so the arm `gc-root-dominance.yml` gates on could not
  see it. **The checker was the wrong one.**

  Membership in `POLL_CAPABLE_RUNTIME` is by exact emitted symbol. The set
  carried `js_object_get_field_by_name` next to `js_object_set_field_by_name`,
  which reads as symmetric coverage of property access and is not: codegen
  emits the SET verbatim (27 call sites in perry-codegen, 205 in the gate
  corpus) and **never emits the GET at all**. Every property read lowers to
  `js_object_get_field_by_name_f64`, `js_object_get_field_ic_miss` or
  `js_typed_feedback_object_get_field_by_name_f64` — 1324 / 532 / 556 calls in
  the same corpus — and not one of the three was in the set. Property sets
  classified `MOVING: YES`; property gets classified `MOVING: no`. That
  asymmetry is what put "all five violations the gate reports are `MOVING: YES
  via js_object_set_field_by_name`" in the rooting-invariant doc, and it left
  the whole GET side unread.

  The premise for each addition is the one the set already granted the name it
  could not match: `js_object_get_field_by_name` resolves a user getter through
  a transmuted function pointer
  (`object/field_get_set/get_field_by_name.rs:1064`) and routes a registered
  Proxy to `js_proxy_get` (`:84`) — arbitrary JS with its own back-edge polls.
  `js_object_get_field_by_name_f64` (`ic_miss.rs:29`),
  `js_object_get_field_by_name_boxed` (`:81`),
  `js_object_get_field_by_property_id_f64` (`:123`),
  `js_object_get_field_ic_miss` (`:228`, `:275`, and `:252` to `js_proxy_get`),
  `js_object_get_field_ic` (`:526`, `:544`, `:561`) and
  `js_typed_feedback_object_get_field_by_name_f64` (`typed_feedback.rs:992`)
  all reach it.

- **`scripts/gc_root_dominance_check.py`: ten `POLL_CAPABLE_RUNTIME` entries
  named symbols that do not exist.** Auditing the set the way #7227 audited
  `ALLOC_RE` found that TEN of its twenty-eight entries matched no
  `extern "C" fn js_*` anywhere in perry-runtime or perry-stdlib:
  `js_apply_function`, `js_array_for_each`, `js_array_sort`,
  `js_call_closure`, `js_call_value`, `js_function_call`, `js_invoke_closure`,
  `js_object_get_property`, `js_object_set_property`, `js_string_replace`.

  Four of the ten were four different spellings of "call a JS closure", so the
  most obviously poll-capable operation in the language was covered zero times
  — while `RECEIVER_SINKS`, three hundred lines away in the same file, already
  spelled it `closure_call\w*`. Each phantom is **replaced** by the emitted
  symbol carrying its premise, never merely deleted (deleting turns the audit
  green and leaves the hole): `js_closure_call0`–`16`,
  `js_closure_call_array`, `js_closure_call_apply_with_spread`
  (`closure/dispatch/calln.rs:33`); `js_native_call_value`
  (`closure/dispatch/value_call.rs:17`); `js_array_sort_default` /
  `js_array_sort_with_comparator` (`array/sort.rs:542,635`);
  `js_typed_array_for_each` (`typedarray/iterate.rs:137`);
  `js_object_get_property_key` / `js_object_set_property_key` /
  `js_object_set_property_key_method` (`object/property_key.rs:154,210,257`);
  and the four `js_string_replace_*_fn` variants that take a user replacer
  callback. The `_dyn` and plain `string_replace` variants are a separate
  coverage decision with their own hit count and are deliberately left out,
  same reasoning as `ALLOC_RE`'s deleted `bigint_\w+_op`.

### Added

- **`scripts/gc_root_dominance_check.py --audit-poll-capable`, and a
  `gc-root-dominance.yml` step for it.** `--audit-alloc-re` (#7227) exists
  because a regex alternative matching nothing reads as coverage. This is the
  same hazard on the other axis and a sharper one: `ALLOC_RE` decides whether a
  register *has* a heap-value source, `POLL_CAPABLE_RUNTIME` decides whether
  the window around it is *moving*, and the moving classification is what the
  two `--moving-only` gate arms actually key on. A phantom entry is invisible
  even to a careful reader, because the misspelling is a plausible name for a
  real operation. The audit fails on any entry that names no exported
  `extern "C" fn js_*`, is static and instant, and runs before the build next
  to the ALLOC_RE audit. It is itself self-tested in both directions, and the
  self-test additionally pins the three GET-family names in place by name.

- **A `--self-test` fixture pair for the property-GET window.** Reduced from
  the real emitted IR (`property_get/generic_dispatch.rs`'s `pget.recv_ok`
  block, and the full-outline `js_object_get_field_ic` at
  `object/field_get_set/ic_miss.rs:544`): a bound shadow slot is loaded into a
  bare register, the generic GET dispatch runs, and the register is
  dereferenced below it. The control re-reads the slot below the dispatch and
  must report nothing. The arm asserts the raw report *and* survival under
  `--moving-only`, because the raw arm was never the broken one.

## Verification

No Rust changed — `git diff --stat` is three files, all Python / YAML /
Markdown — so `cargo test` is untouched by construction rather than by
measurement.

Corpus: 141 `.ll` files, 2321 functions, 3068 root stores, emitted by
`./scripts/gc_root_dominance_corpus.sh` from a build of this worktree.

### The adjudication, measured

| over the gate corpus | parent (`97c69211d`) | this PR |
|---|---|---|
| compiled functions in `poll_reaching` | 473 | **671** |
| `--stale-registers`, raw | 4860 | 4860 |
| `--stale-registers --moving-only` | 65 | **115** |
| `--stale-registers --fatal-sinks` | 1055 | 1055 |
| `--stale-registers --moving-only --fatal-sinks` | 1 | **13** |
| bind-anchored `--moving-only` (**the gate**) | 0 | **0**, allowlist still empty |
| `--unrooted-allocas --moving-only` (**the gate**) | 0 | **0** |

The raw counts do not move, which is the point: this change reclassifies
windows, it does not widen what counts as a heap-value source. 55
`--stale-registers` uses have an emitted property-GET helper in their window
and 31 of them classified `MOVING: no` before this change, so `--moving-only`
dropped all 31.

The twelve newly moving-*and*-fatal leads, triaged by shape (diagnostic mode,
not gated):

| sink | count | classified moving via |
|---|---|---|
| `js_object_set_field_by_name` | 6 | `js_object_get_field_by_name_f64` |
| `js_set_function_prototype` | 3 | `js_object_get_field_by_name_f64`, `js_object_get_field_ic_miss` |
| `js_array_push_f64` | 1 | `js_object_get_field_by_name_f64` |
| `js_closure_call1` | 1 | `js_closure_call1` |
| `js_typed_feedback_closure_direct_call_guard` | 1 | `js_closure_call1` |

### The gate can now fail on the shape

The self-test arm goes red against the parent's `POLL_CAPABLE_RUNTIME` and
green with this one:

```
$ python3 <parent set + this PR's arms> --self-test
self-test FAIL: the generic property-GET dispatch resolves a user getter
(object/field_get_set/get_field_by_name.rs:1064) and routes a Proxy to
js_proxy_get (:84), so a register held across it MUST classify MOVING and
survive --moving-only. Got {} ...
exit 1
```

And the gate's own command, over a corpus mutant that splices exactly one
`js_object_get_field_ic_miss` into one real allocation-to-root-store gap —
nothing else changed:

| `--moving-only --allowlist … --seeded-violations 40` over the mutant corpus | parent | this PR |
|---|---|---|
| violations | 0 | **1** |
| exit status | **0 (green — blind)** | **1 (red)** |

```
test_gap_array_iterator_next…::perry_closure_…__1
  alloc  : %r15 = call i64 @js_closure_alloc_singleton(...)
  store  : store double %r17, ptr %r14
  bind   : slot 3  call void @js_shadow_slot_bind(i32 3, ptr %r14)
  between: js_object_get_field_ic_miss
  MOVING : YES via js_object_get_field_ic_miss
```

Complementary direction, on the unmutated corpus: reseeding
`--seeded-violations`' spliced collection point with
`js_object_get_field_ic_miss` instead of `js_call_function` gives
`40 planted, 0 caught, 40 MISSED` on the parent and `40 planted, 40 caught`
here.

### The two hypotheses that were NOT the answer

Both were investigated before editing, and both are recorded because ruling
them out is most of the adjudication.

- **"Attribution mismatch — the register goes stale in a caller and arrives as
  an argument."** `heap_source_kind` genuinely has no incoming-parameter source
  kind, so a stale *parameter* register would be invisible in the callee. It is
  a real gap and it is not this one: perry spills and binds parameters
  (`store double %argN, ptr %rM` + `js_shadow_slot_bind`), which makes the
  re-read a `slotload` source the checker already models. Measured over the
  corpus, 1708 parameters are spilled and 256 are not; of the 256, exactly 4
  are used below a collecting call and all 4 are `double`/`i64` arithmetic in
  typed-f64 specialisations, none of them a pointer. That gap needs its own
  change with its own measured hit count; the new fixture passes its receiver
  and key as bare parameters so the shape is at least on the record.

- **"Protector overreach — DEPTH=800 proves staleness against *some* prior
  collection, not this window."** Structurally impossible. A page-set enters
  the quarantine only because an evacuating minor retired it as from-space
  (the knob gates `copying_reset_from_spaces_and_flip` and nothing else,
  `arena/quarantine.rs`), so any fault on a quarantined address is a genuine
  stale read at any depth. Eviction hands blocks **back to Eden**, where the
  same read silently succeeds — so depth removes false negatives and cannot
  manufacture a false positive. "It only faults at DEPTH=800" is never grounds
  to doubt the protector. The rooting-invariant doc now says so.
