### perf(object/json): `JSON.parse` receivers reach both object-write fast paths (#8098)

A `JSON.parse` object carries `class_id == 0`. Both guarded object-write fast
paths — the whole-loop numeric clone and the static/dynamic write PICs — rejected
it on exactly that, so every `record.field = …` on parsed data took the generic
`[[Set]]` path for the life of the program. `JSON.parse` is how essentially all
external data enters a Perry program (HTTP bodies, config, ORM rows, cache reads,
IPC payloads), and the rejection was on the *receiver's identity*, so no amount of
loop-shape or key-form work in #6812 could reach it.

Measured on the committed `benchmarks/object-write-6812/matrix.ts` controlled
pair, which differs in exactly one way (`JSON.parse('{"x":0}')` instead of an
object literal) and produces an identical `sink 122876400` over identical
120,000,000 writes. Wall clock on the development host is unusable (the same cell
measured 11.7 s / 17.9 s / 21.1 s across three runs), so the ratio is reported in
**instructions retired**, which reproduced to within 0.02%:

| cell | instructions retired | vs `key_dot` |
|---|--:|--:|
| `key_dot` (object literals) | 1.158e9 | 1.00x |
| `receiver_class_id_zero` before | 150.08e9 | **129.5x** |
| `receiver_class_id_zero` after | see below | |

**Why the guard could not simply drop the clause.** `class_id != 0` was standing
in for three per-object exclusions that the generic path still applies verbatim
(`object/field_set_by_name/fast_paths.rs::try_existing_own_data_overwrite`):
`NATIVE_MODULE_CLASS_ID`, `Object.prototype`, and a `URL` instance — whose
`pathname`/`search`/… own slots are live views whose setters rebuild `href`
(`field_set_by_name/tail.rs`). None of those is derivable from the ShapeId: two
objects share a ShapeId iff they share a keys-array *allocation*, and the
shape-transition cache deliberately converges distinct objects onto one shared
array, so a prime-time-only exclusion loses to ordering (a plain object primes the
site, a `URL` that later acquires the same keys array then hits it). The
generated hit path re-checks only per-object state, so the discriminator has to
be per-object too.

**What landed instead** is an explicit, opt-in, per-object mark:
`OBJ_FLAG_PLAIN_ORDINARY` (bit 9 of `GcHeader::_reserved`, object-only, disjoint
from the array-only `GC_ARRAY_ARGUMENTS_OBJECT` by `obj_type` the same way bits 11
and 12 already are). The JSON direct parser and the lazy-tape materializer set it
at birth; every other class-less receiver is unmarked and keeps the full `[[Set]]`
walk, so no existing population changes behaviour. The bit is free in the
generated guard — `_reserved` is already loaded there for the blocking-flag test,
so admission costs one `and` + `icmp` + `or`, hoisted above the four PIC ways.

Note that the *read* PIC has admitted `class_id == 0` all along
(`object/field_get_set/ic_miss.rs` primes on any regular descriptor-free shaped
receiver, and the emitted read guard has no `class_id` compare at all). Reads of
parsed objects were already on the ShapeId fast path; only writes were not. #8067
/ #8086 supplied what was missing on the write side: a parsed receiver is
birth-stamped with a real ShapeId by `js_object_alloc_class_inline_keys`, and
repeated parses of one shape share a single `GC_FLAG_SHAPE_SHARED` keys array via
`PARSE_SHAPE_CACHE`, so the whole 2400-receiver prefix carries one ShapeId.

Also fixed here, in the same file: `JSON.parse("{}")` initialized **eight** inline
field slots into an allocation that has `max(0, INLINE_SLOT_FLOOR)` = **two** of
them (the floor dropped 4 → 2 in #7928) — a 48-byte overwrite past the object on
every empty-object parse, the exact "heap buffer overflow into adjacent arena
objects" that `js_object_alloc_with_parent` documents. The hand-rolled fill was
redundant as well: the allocator has initialized every slot it allocates since
#4717.

Coverage:

* `crates/perry-runtime/src/proxy.rs` —
  `json_parse_receivers_are_admitted_to_the_whole_loop_write_clone` and
  `json_parse_receivers_prime_the_static_write_pic` drive the shipped
  `js_json_parse` end-to-end (payloads are a few bytes with an object root, so
  the eager direct parser runs and no lazy tape stands between the probe and the
  objects — #7635), assert the premises (`class_id == 0`, a real shared ShapeId),
  and then clear the mark on one receiver and require the guard to refuse. Both
  fail when the guard ignores the mark and when the parser stops setting it.
  `plain_ordinary_object_flag_matches_the_emitted_write_pic_literal` pins the bit
  value against the literal `perry-codegen` emits.
* The pre-existing `object_array_numeric_write_guard_requires_complete_uniform_proof`
  keeps its class-id-zero rejection for an *unmarked* receiver and gains the
  marked-accepts and native-module-still-rejects halves.
* `test-files/test_gap_json_parse_object_writes.ts` — parity against node 26.5.1
  for the semantics the `class_id != 0` clause used to keep parsed objects away
  from: deleted keys, added keys, frozen/sealed/non-extensible receivers (strict
  `TypeError`s), accessor and non-writable descriptors installed over a parsed
  slot, prototype mutation with a shadowing setter, null prototypes, dynamic-key
  writes, a parsed object used as a prototype, an empty parsed object grown by
  name, a polymorphic site mixing parsed objects / literals / class instances,
  and `__proto__` / `constructor` as genuine own data keys.
