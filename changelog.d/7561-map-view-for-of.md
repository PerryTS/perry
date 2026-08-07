### Performance

**`for (… of m.values() / m.keys() / m.entries())` is now the same
allocation-free index walk as `for (const [k, v] of m)` (#7561).** `map_1m` was
the largest absolute time in the public benchmark artifact — 1233.7 ms against
bun's 256.5 and node's 320.1, nearly double the next-slowest kernel — and
nobody had looked at it. Profiling first says the Map was never the problem.

Phase decomposition on the pinned quiet mini (5 interleaved runs, load 1.52,
node 26.5.1 / bun 1.3.14):

| phase | before | after | node | bun |
|---|--:|--:|--:|--:|
| insert 500k `m.set("key_" + i, …)` | 220 ms | 219 ms | 140 | 97 |
| look up 500k `m.get("key_" + i)` | **76 ms** | **76 ms** | 115 | 103 |
| **`for (const v of m.values())`** | **512 ms** | **5 ms** | 4 | 6 |
| 10k `m.has("missing_" + i)` misses | ~1 ms | ~1 ms | 5 | 3 |

Perry already **won lookup** — 76 ms against node's 115 and bun's 103 — and won
the miss loop. Insert is 1.6× node, and its symbolicated profile puts 55.5% in
`js_string_concat_value` building the keys, with 45.5% of the whole probe inside
the `gc_collect_minor_with_trigger` those string allocations trigger;
`find_string_key_index` is 17.25%. That is the #7469 allocation path, not a Map
story. **Iteration was 128× node**, and it was the entire gap.

Inside it (9679 leaf samples, `PERRY_DEBUG_SYMBOLS=1`): `.values()` produced a
real Map **iterator object** and drove it through the generic protocol, one
`.next()` per element.

| stage | inclusive |
|---|--:|
| `js_typed_feedback_native_call_method_by_id` — the per-element `.next()` | **76.37%** |
| └ `dispatch_handle` → `dispatch_map_iterator_method` | 62.91% |
| **└ `make_iter_result` — building `{ value, done }`** | **55.08%** |
| ├ `js_array_push_f64` (the two keys) → `note_array_slot` → `invalidate_representation_change` → `pthread_mutex_lock` | 22.09% (10.96% in the mutex) |
| ├ `string_storage_alloc` — allocating `"value"` and `"done"`, **per element** | 16.05% |
| ├ `js_array_alloc` (the keys array) | 9.00% |
| ├ `js_object_set_field` ×2 | 7.09% |
| ├ `js_object_alloc_with_parent` — the result object | 5.09% |
| └ `shape_id_for_keys_ensure` | 4.99% |
| `gc_check_trigger` → `gc_collect_minor_with_trigger` (driven by the above) | 16.59% |
| `js_object_get_field_ic_miss` — reading `.value` / `.done` back off it | 12.18% |
| `_tlv_get_addr` | 12.98% |

**Five heap allocations per element** — result object, two key strings,
two-element keys array, shape install — 2.5 M of them for one sweep of a 500k
Map. The identical iteration written directly, `for (const [, v] of m)`,
**already cost 5 ms**: it lowers to a delete-safe index walk over the flat
entries buffer and allocates nothing. The lowering keyed on the *syntax* of the
for-of subject rather than on what it produces, so 161× separated two spellings
of one loop.

`for_head::rewrite_collection_view_for_of` now rewrites the view call to the
direct-collection form when the receiver's static type proves `Map` / `Set`
**and** the resulting head is one the index fast path accepts:

| subject | head | becomes |
|---|---|---|
| `m.entries()` | any index-fast-path head | `for (<head> of m)` |
| `m.keys()` | plain ident `k` | `for (const [k] of m)` |
| `m.values()` | plain ident `v` | `for (const [, v] of m)` |
| `s.values()` / `s.keys()` | plain ident | `for (<head> of s)` |

The gate is deliberately narrow, and the last clause is the load-bearing one.
`for (const [a, b] of m.values())` destructures the *value*, not the entry;
`s.entries()` yields `[v, v]`, which `for (… of s)` does not; `for await` needs
its per-iteration await. And **any head the index walk would decline is left
alone too** — because the Map/Set fallback is a `MapEntries` / `SetValues`
materialisation, i.e. a snapshot, so rewriting onto it would change what a body
that mutates the collection observes.

Results on the pinned quiet mini, `hyperfine --warmup 3 --runs 15`:

| | before | **after** | node 26.5.1 | bun 1.3.14 |
|---|--:|--:|--:|--:|
| **`map_1m` (the kernel)** | 820.0 ± 5.1 ms | **309.1 ± 2.7 ms** | 323.1 ± 1.6 | 221.0 ± 0.9 |
| insert + `for (const v of m.values())` | 952.4 ± 3.3 | **231.3 ± 0.7** | 205.1 ± 1.9 | 121.0 ± 0.9 |
| insert + `for (const e of m.entries())` | 1296.7 ± 3.4 | **234.2 ± 0.5** | 206.4 ± 1.4 | 123.4 ± 0.7 |

**2.65× on the kernel, and Perry now beats node on it** (309 vs 323) where it
was 2.5× node before; perry/bun moves from **3.71× to 1.40×**, and what is left
is the insert phase's string construction. Per-loop, each measured in its own
process over a 500k Map: `m.values()` 770 → 5 ms, `m.keys()` 978 → 6 ms,
`m.entries()` 1128 → 7 ms, `for (const e of m)` 1265 → 6 ms, `s.values()`
370 → 5 ms, and the untouched `for (const [k, v] of m)` control flat at 5 ms.

`PERRY_GC_TRACE=1` whole-process totals: `map_1m` goes from **8 collections
(7 minor + 1 full) to 2 minors and no full**, the values sweep from 6 (3+3) to
2, the entries sweep from 9 (4+5) to 2. Copied objects (445,639) and copied
bytes (17.8 MB) are **unchanged** — the surviving live set is identical and only
the iteration's garbage is gone — and both arms still run copying minors with
`copied_objects > 0`, so the comparison is not the vacuous kind.

### Fixed

**`for (const k of m.keys())` could hand the loop body a moved string
(#7561).** The route it took, `make_iter_result`, allocates the strings
`"value"` and `"done"` on every `.next()` while the caller's `value: JSValue`
sits in a bare register the collector cannot see or rewrite. A probe over
40 000 heap-string Map keys that allocates inside the loop body already gets a
**wrong answer with no instruments at all** (one mismatched key, and a `chars`
total of 3,944,095,011 where 1,548,890 is correct); under
`PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1
PERRY_GC_PROTECT_FROMSPACE_DEPTH=800` on a `PERRY_GC_MOVING_LOOP_POLLS=1`
build, with the instrument proven live by its
`[gc-fromspace-protect] mode=ProtectPages retired_set=#0 blocks=17` line, it
takes a bus error whose reporter names the site exactly:

```
[gc-fromspace-protect] FAULT: signal 10 at 0x5b6f1f8ffe4
  This address is RETIRED FROM-SPACE. …
  last-known object: user_ptr=0x5b6f1f8ffd8 obj_type=3 size=40
2  perry_runtime::iterator_helpers::make_iter_result + 172
4  object::native_call_method::handle_methods::dispatch_handle + 4376
```

The index walk does not build a result object, so the defect is unreachable for
`for (… of map/set …)` after this change: the same probe under the same flags
is `mismatch: 0` with **zero faults** and the instrument still live
(`retired_set=#5`), and a 150 000-key variant is clean across 4 copying minors
— one moving 150,014 objects — plus a `PERRY_GC_VERIFY_EVACUATION=1` pass. It
is **not** fixed for generators, custom iterators, `yield*` or spread, which
still drive `make_iter_result`; that half is #7564.

**A single-ident Map for-of head bound a snapshot, not the Map (#7561).**
`for (const e of m)` went through `MapEntries`, which materialises the whole Map
up front, so the loop could not see its own body's writes:

```ts
for (const e of m) { …; if (first) m.set("late", 9); }     // node a,b,late — perry a,b
for (const e of m) { …; if (e[0] === "a") m.delete("b"); } // node a,c      — perry a,b,c
```

The index fast path now accepts that head and binds one fresh `[key, value]`
pair per step, read out of the entries buffer at the delete-corrected index —
the same allocation node's Map iterator makes, and live. Both cases now match
node, and the shape is also ~180× faster at 500k entries.

### Notes

Both commits dedup what the two for-of desugars (`lower/stmt_loops.rs` for
module scope, `lower_decl/body_stmt.rs` for function bodies) carried
byte-identical copies of — the #302/#311 iterable-type resolution and the
fast-path head predicate. `stmt_loops.rs` 2000 → 1954, `body_stmt.rs`
2140 → 2092.

A 115-line Map semantics matrix — insertion order, overwrite-in-place,
delete-then-reinsert, SameValueZero (`NaN`, `-0`/`+0`), string-representation
boundaries (SSO / heap / multibyte / surrogate pair / NUL / `__proto__`), object
vs string key identity, iterator invalidation under append and under
delete-ahead / delete-current / delete-behind, `Symbol.iterator` vs `entries()`
vs `forEach` agreement, `clear()` and reuse, mixed key kinds, union-typed
receivers, class fields, object properties, `break` / `continue` /
labeled-break / `return`, `let` and `var` heads, nested loops — is
byte-identical before and after except the four lines the fix deliberately
changes, and matches node everywhere except two divergences that predate this
work and are unchanged by it (`Map.prototype[Symbol.iterator] ===
Map.prototype.entries` is `false`; `for (var k of …)` does not leak `k`).
`test-files/test_gap_map_view_for_of.ts` is byte-identical to node after and had
two divergences before.

`lower/collection_view_tests.rs` pins the **verdict** — which of the three
lowerings (index walk / materialisation / iterator protocol) each loop shape
receives — not the output. The iterator path is a correct fallback, so a test
comparing program output alone would stay green if the fast path silently
stopped applying (CLAUDE.md's fourth way a gate can be unable to fail); a
`the_route_probe_actually_discriminates` case proves the probe can still tell
them apart.

A patched `Map.prototype.values`, a patched `Map.prototype[Symbol.iterator]`,
and an own-instance `values` shadow are **already** ignored by Perry before this
change — verified against node, which honours all three — and the pre-existing
direct `for (… of m)` fast path has the same property. A `Map` subclass types as
`Type::Named`, never `Type::Generic`, so an overriding subclass method is never
bypassed.

**Measurement trap worth recording.** The first A/B ran all nine loop shapes in
one process and reported two apparent 2× regressions in shapes this change does
not touch (`for (const e of m.entries())` 570 → 1180 ms, `[...m.values()]`
115 → 240 ms). Re-measured one shape per process, both are flat (1128 → 1124,
92 → 92). Removing ~2.5 M allocations from the *earlier* benches moves the whole
GC regime the *later* ones run in — a multi-bench file stops being a valid A/B
instrument the moment one of its benches changes its allocation rate.

Three defects found while profiling, filed rather than folded in, all
reproducing identically at `969b447cc`: spread / `Array.from` over any iterator
silently truncating at 100,000 elements (#7562), a SIGSEGV iterating a
`values()` override on a `class X extends Map` (#7563), and `make_iter_result`
allocating five objects per `.next()` where four are the same constant every
call (#7564) — the general form of what this change routes around for Map and
Set, still paid by every generator and custom iterator, and carrying the
rooting defect above on that path too.
