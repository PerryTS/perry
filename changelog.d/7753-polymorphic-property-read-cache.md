### perf(codegen, runtime): property reads resolve inline for polymorphic receivers, and `arr.length` stops walking the object ladder (#7753)

A tree-walking interpreter (`gc-handoff/apps/interp.ts` — lexer, precedence-climbing
parser, recursive `let`, closures over environments; 189 statements, no dynamic
remainder) ran **3.96 s** against Node 26.5.1's 0.32 s. That is 12.3× Node, three
times worse than any synthetic benchmark in the corpus (all now 2.3–4.0×) and 2.3×
worse than scriptc. It is also the only program in the corpus that resembles real
software. It is now **2.39 s**, a 1.66× speedup, from two changes with a single root
cause: *Perry could not cache a property read whose receiver had more than one shape.*

GC is not involved — 0.04 s of pause across the whole run, 38 minors, zero fulls.

#### What the cost actually was

Cutting the program down (`gc-handoff/bench/b_fib.ts`, FIB only) isolated **3.88 s of
the 3.96 s** into one interpreted program, and a symbolicated profile put **34% of it
in `js_object_get_field_ic_miss`** and the ladder it calls. The reason is structural:
the per-site cache is a **single entry**. `evalNode` dispatches on
`n.kind === "num" | "str" | "var" | "bin" | "if" | …`, so the receiver at those sites
cycles through five shapes and the one entry is wrong on essentially every read. Each
miss then re-derives the receiver kind from scratch — proxy band, closure magic, the
registered-buffer and typed-array registries (both behind thread-locals), the
accessors-in-use latch — and finishes with a linear scan of the keys array doing a
`js_string_equals` per key.

This also explains the profile lines that made no sense on their face:
`typedarray::lookup_typed_array_kind` at 3.4% and `buffer::header::is_registered_buffer`
at 1.9% **in a program that uses neither typed arrays nor Buffers**. They were not a
separate problem; they were inside the miss handler.

Three hypotheses were tested and refuted before this one, and the refutations are
worth keeping because each looked obviously right:

* **String-keyed union tags are slow.** `js_jsvalue_equals` + `from_utf8` + `memcmp` +
  `js_string_equals` came to 23% of the profile. Refuted: `bench/tag_str.ts` vs
  `bench/tag_num.ts` are both 0.06 s. Converting the whole interpreter to numeric tags
  (`bench/a_numkind.ts`) moved 3.88 s → 3.71 s — 4%. Most of that 23% was the *keys
  scan inside the miss handler*, not the user's `===`.
* **Runtime-built heap strings compare slowly.** Refuted: `bench/streq_sub.ts` is 0.05 s
  vs Node's 0.07. Interning every identifier at lex time (`bench/a_intern.ts`) moved
  3.88 s → 3.87 s, and numeric opcodes (`bench/a_numop.ts`) → 3.89 s. Neither is
  measurable.
* **A megamorphic cliff at some shape count.** Refuted by a clean sweep holding the
  loop body and array size fixed (`bench/meg{2..12}.ts`): a flat 3.0–3.7× at every
  arity. Read as "no cliff, so not the cause" this is misleading — flat-and-already-3×
  from **two** shapes onward is precisely the signature of a one-entry cache that
  misses from the second shape on. The arity sweep could never show a cliff because
  there is nothing to fall off.

The discriminating probe was `bench/a_flatnode.ts`: the same interpreter with every AST
node built from one object shape, so every `n.*` read is monomorphic — recursion depth,
allocation count, string traffic and the environment chain all held constant. 3.88 s →
2.84 s. That put a number on the polymorphism itself and is what this change went after.

#### Change 1 — the read cache gets polymorphic ways

`@perry_ic_N` widens from `[8 x i64]` to `[12 x i64]`: the MRU entry `[token, slot,
epoch]` keeps its exact pre-existing meaning, followed by four `(token, slot)` ways and
a victim counter. On a miss the handler no longer discards the shape it evicts — it
cascades it into a way, so a site alternating between up to five shapes ends up with all
five resolvable inline.

The emitted way compares live **inside the miss block, below the feedback records and
above the call**. A monomorphic site therefore executes the identical instruction
sequence it did before; the new work is reached only by a site that was already going to
call the runtime. Typed-feedback counters are recorded before the compares, so a way hit
still reports guard-fail + fallback-call exactly as it did when it was a real miss — the
heuristics see an unchanged signal (the site *is* polymorphic; only its cost changed).

Two things had to be got right, and both were found by a test rather than by reasoning:

* **The MRU token must be evicted from the ways when it is promoted.** Otherwise a way
  permanently duplicates word 0 and a k-shape rotation only ever caches k−1 shapes.
  `alternating_shapes_all_become_inline_resolvable` caught this.
* **The ways must accept keys-POINTER tokens, not only shape-ID tokens.** ID tokens
  (#6804) need no epoch validation, which makes an ID-only way set look like the safe
  design. It is also useless: a plain object literal is built through a generated
  `__AnonShape_*` constructor and so carries a real `class_id`, which routes it to the
  shape-shared keys-pointer prime. Shipped that way it was a **6% regression** — the
  compare sequence running on every miss and never once hitting. `pointer_tokens_do_reach_a_way`
  is the test that fails if it is narrowed again.

Admitting pointer tokens means the ways inherit #6080a: a keys-array address freed by a
collection can be recycled under a different shape, and a stale way would pointer-match
and load the wrong slot **silently**. The ways therefore share word 2's epoch snapshot
with the MRU entry — the emitted way predicate requires `cache[2] == @PERRY_IC_EPOCH`,
and `pic_prime_get` **wipes every way whenever it writes a new epoch**, dropping the
evicted token as well (it too was resolved in the old epoch). A readable way is thus
always one primed in the epoch word 2 still holds. The ways go cold once per collection
— 38 of them across a 4 s run — and re-prime. `an_epoch_change_wipes_every_way` asserts
both halves.

`interp.ts` 3.96 s → **3.01 s**. `js_object_get_field_ic_miss` fell from 9.0% of the
profile to 1.0%.

#### Change 2 — `arr.length` short-circuits in the miss handler

With `evalNode` fixed, the *entire* remaining miss cost moved to one place: 1143 of 5241
leaf samples, all from `lookup`, and none of it a polymorphic object read. It was
`names.length`.

The inline cache requires a `GC_TYPE_OBJECT` receiver by construction (#72 — so an
Array's `element[1]` is never mistaken for `keys_array`), which means **every** dynamic
`.length` misses, permanently, by design. It then walked a ladder built for objects: a
closure-magic deref, two registry probes behind thread-locals, then
`js_object_get_field_by_name`'s own dispatch, which repeats the registry probes before
finally reaching the array arm. For a variable lookup written the ordinary way —
`for (i = 0; i < names.length; i++)` — that single read was **22% of total run time**,
more than the polymorphic fix above had saved.

`js_object_get_field_ic_miss` now answers it directly when the receiver's `GcHeader`
says `GC_TYPE_ARRAY`. That type is a genuine dense array: buffers, typed arrays, lazy
arrays, Sets and Maps all carry distinct `obj_type`s, and a `class X extends Array`
instance is an `ObjectHeader`. `js_array_length` still resolves growth-forwarding stubs,
proxies and subclass receivers, and the returned expression is the one
`get_field_by_name_object_tail`'s array arm already computes for this key — so this is a
short-circuit, not a second implementation.
`array_length_short_circuit_agrees_with_the_full_ladder` pins that by comparing against
`js_object_get_field_by_name_f64` for empty, small and grown arrays, for a same-length
key that is *not* `length`, and for `length` on a plain object.

`interp.ts` 3.01 s → **2.39 s**.

#### Measurements (quiet M1 mini, best-of-5, absolute seconds, outputs byte-identical to `node --experimental-strip-types` before timing)

| program | before | after | |
|---|--:|--:|---|
| `interp.ts` | 3.96 | **2.39** | 12.3× → 7.4× Node |
| `b_fib.ts` (reduced case) | 3.88 | **2.32** | |

Protected floors, all held, each also A/B'd against the same-host v0.5.1434 build:

| | churn | churn_alloc | push_cls | push_num | churn_read | cycles | deeplist | tree | tree_wide | retain | retain_wide | fib40 |
|---|--|--|--|--|--|--|--|--|--|--|--|--|
| after | 0.42 | 0.36 | 0.35 | 0.13 | 0.02 | 0.19 | 0.24 | 1.65 | 2.11 | 0.53 | 1.08 | 0.39 |
| floor | 0.42 | 0.38 | 0.36 | 0.15 | 0.03 | 0.20 | 0.26 | 1.67 | 2.15 | 0.56 | 1.12 | 0.41 |

`gc-handoff/apps/iso_miss.ts` prints `checksum 437840 misses 0` — gated on the miss
counter, not the aggregate, because a perf change has previously made `interp.ts`'s
total read correct while a silent-wrong-answer GC bug (#7682) was fully intact. Also
clean under `PERRY_GC_VERIFY_EVACUATION=1` and
`PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`, which matter here
because the ways hold raw heap addresses in a global no GC scanner can see.

#### What is left

`interp.ts` is 7.4× Node, not the ~5× scriptc reaches. The remaining profile is
`js_jsvalue_equals` (15.7% — `===` is still a runtime call per comparison, and an inline
fast path only resolves the ~20% of comparisons that are *true*), the per-object GC
layout tables on the allocation path (7.7%, the #7510/#7469 area), write barriers (5.9%)
and `js_dyn_index_get`'s registry probes (~3%, the same "route by `GcHeader` before
probing address registries" shape fixed here twice). None of those is a
single-mechanism gap the way this one was.
