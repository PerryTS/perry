perf(ic): answer a dynamically typed `arr.length` from the Array's GC header at
the call site, instead of calling out to the inline-cache miss handler (#10714).

A `.length` read whose receiver codegen cannot prove is an Array lands in the
generic property-get tower, whose inline cache requires a `GC_TYPE_OBJECT`
receiver by construction (#72). An Array can never match it — its `+4` word is
`capacity`, which rule 3 (#10828) bounds below the ShapeId floor — so every such
read called `js_object_get_field_ic_slow`, walked `get_field_ic_miss_impl`'s
ladder to its `GC_TYPE_ARRAY` arm, answered, and cached nothing, because there is
nothing an object cache can hold for an Array. `PERRY_IC_DIAG` on a natively
compiled `tsc --noEmit` counted 1,717,633 of 6,282,240 misses (27.3%) as
`array_length`, and the three hottest miss sites of the run were all `.length`.

Both emitted generic-get sites now test the receiver's GC header and load
`ArrayHeader.length` inline (`emit_plain_array_length_arm` in
`expr/property_get/generic_dispatch.rs`):

- **Inline tower**: a `.length` site's ShapeId compare branches its FALSE edge
  to `pget.array_kind` (`obj_type == GC_TYPE_ARRAY` and `GC_FLAG_FORWARDED`
  clear — both header bytes, one flat predicate) and on to a `pget.array_length`
  load; anything it refuses continues to `pic.token.miss` exactly as before. The
  test sits after the compare, not ahead of it the way `.size`'s Map/Set arm
  does, so a `.length` site's object HIT path is the same instruction sequence
  every other key's is (pinned by `a_length_site_hit_path_reads_no_gc_header`).
- **Full-outline modules** (#5391 path 3, ≥4000 callables): the same header
  test, behind an exact-POINTER-tag and small-handle-band check, answers a live
  plain Array ahead of the `js_object_get_field_ic` call; every other receiver
  takes the call unchanged.

**Forwarding stubs (inline tower only).** A plain-array test alone left a large
remainder on tsc: an array that grows past its capacity moves, and a binding the
growing code did not write through — a field holding the array while
`obj.items.push(x)` grew it, an alias — keeps the old head, now a stub
(`GC_FLAG_FORWARDED`, the live head's address in the first payload word, where
`length` was), until a collection heals it; a loop that allocates nothing never
collects. So on the inline tower's refusal edge only, a stub follows ONE edge
exactly as `index_get/guarded_array.rs` already does: the forwarding word is
trusted only once it is a heap address above the handle band, and the
destination is re-checked as a live plain Array before its `length` is read. A
longer or malformed chain still goes out of line, where `clean_arr_ptr` follows
it and compresses it to one edge, so the next read heals inline. A live plain
Array's read is the header test and the load, nothing more. The full-outline
site does not follow stubs: that mode exists to keep each site small, and with
the three extra blocks its all-live-arrays loop measured ~1450 ms against
~1040 ms without them (same hot-path instructions; the larger CFG changed the
loop's register allocation), so a stub there takes the helper call as before.

The answer is the runtime's: for a non-forwarded `GC_TYPE_ARRAY`,
`js_array_length`/`clean_arr_ptr` return the `u32` at payload offset 0, sparse
arrays included, and a primitive array's `length` is non-configurable. Buffers,
typed arrays, lazy JSON arrays, Maps and Sets carry their own `obj_type`, and an
Array subclass instance is a `GC_TYPE_OBJECT`, so none of them can take the arm.
This is the header test the proven-Array lowerings already inline
(`plen.check_gc`, `kindguard.array_header`, `apop.hdr`). `--typed-feedback`
builds keep the call for every receiver, so their guard-fail/fallback-call
records (and, when full-outlined, the helper's OBSERVE) stay byte-identical — the
trade the inherited-read hook already makes.

Measured (x86-64 Linux, 4 cores; release compiler, both arms built from the same
tree, interleaved runs, medians):

| | before | after |
|---|---:|---:|
| `typescript@5.9.3` `_tsc.js` native, `--noEmit` on a 2-line file: `PERRY_IC_DIAG` `array_length` misses | 1,807,854 | 1,392 |
| same run, all IC misses | 7,059,456 | 5,477,632 |
| same run, top `.length` miss sites | 424,427 / 374,545 / 374,545 | none among the 40 reported sites (smallest row: 36,044) |
| same run, wall time (n=6) | 18.07 s | 17.68 s — ranges overlap, within noise |
| 64M `boxes[i].payload.length` reads, all live arrays | 1898 ms | 738 ms |
| same, full-outline (`PERRY_FULL_OUTLINE_IC=1`) | 2099 ms | 1019 ms |
| same loop, all receivers objects with an own `length` (hit-path control) | 735 ms | 725 ms — unchanged |

A plain-array test without the stub follow left tsc at 477,202 `array_length`
misses (380,278 at one site); an array grown through a field in a loop that
allocates nothing missed on every read until the follow was added, and misses on
none now. The tsc wall time does not move measurably, as #10714 predicted: the
miss handler was ~4% of that run. The native tsc's output and exit code are
byte-identical to `node node_modules/typescript/lib/_tsc.js`, and 243 of 244
`test-files` programs matching array/length/JSON/arguments produce identical
output under both compilers (the 244th differs only in an ASLR'd return address
in a printed stack trace).

Tests: `expr/property_get/array_length_tests.rs` pins the arm's placement
(reached only from `pic.token`'s false edge; every refusal continues to
`pic.token.miss`), both header conjuncts and their offsets on the receiver AND on
the forwarding destination, the one-edge follow and its heap-address test, the
load of the proved head feeding the merge phi, no GC-header read anywhere on the
object hit path, no arm for any other key, and the full-outline arm (without the
stub blocks) on a module padded past the threshold. Sabotaging the kind
constant, dropping the forwarded conjunct, disabling the arm, dropping the
follow's handle-band test, or skipping the destination re-check each turns them
red.
`test-files/test_gap_10714_dynamic_array_length_header.ts` reads `.length` at one
generic site on arrays grown through a stale alias (one and two edges deep) and
through a field, sparse, shrunk, extended, shifted, frozen, holey and derived
arrays, JSON arrays, an Array subclass, and — at the same site — strings,
array-likes, typed arrays, Map, Set, functions and nullish receivers.
