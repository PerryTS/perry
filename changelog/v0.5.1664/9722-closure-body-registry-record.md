**Closure body registries: one packed record per function instead of ten
maps keyed by the same pointer (#9707).**

Module init used to record what it knows about each closure body — rest
arity + kind, declared ABI arity, ECMAScript `.length`, arrow / strict /
async / generator / async-generator flags, the two compiler-private
direct-call bodies of an eligible arrow — into ten separate thread-local
`PtrHashMap`s, every one keyed by the same `func_ptr`, plus an eleventh map
memoizing the dispatch strategy those answers imply. Ten key copies, ten
tables' worth of hashbrown load-factor slack, and a dispatch-strategy miss
that probed up to four of them in sequence. On cc's validated heap census
(~59k functions) the family summed to 7.24 MB with the current estimator
(the 11.8 MB the issue quotes was the earlier one, which double-counted
exactly-sized tables).

`CLOSURE_BODY_REGISTRY` is now the single table: `func_ptr →
ClosureBodyRecord`, a 16-byte record holding `.length`, the declared arity
and the rest arity as integers, and every boolean attribute plus the 2-bit
rest kind as flag bits; `(usize, ClosureBodyRecord)` is a 24-byte bucket,
pinned by a size test. The rare `TrustedDirectTarget` pair (direct-call body
and versioned-loop body) moves to a dense append-only side array
(`TRUSTED_TARGETS`) that only eligible arrows index into, so a body without
them pays four bytes, not two `Option<TrustedDirectTarget>` maps. The
dispatch-strategy cache is deleted outright: a miss now does ONE probe of
the record and derives rest/arity/arrow-ness from its bits, which is cheaper
than the second hash probe the cache cost — and cannot go stale, so the
#6475 late-registration invalidation shrinks to the four-entry
`DISPATCH_RECENT` eviction. Every `js_register_closure_*` entry point and
every `lookup_*` / `is_registered_*` reader keeps its signature; rest still
wins over arity for dispatch and `closure_arity`, and `closure_length` still
prefers the explicit length, then rest, then arity.

Measured with `PERRY_GC_CENSUS` on a generated 20k-function fixture (5k
each of default-param arrows, rest functions, async functions and
generators; 35,051 registered bodies including the runtime's own): the
closure registry rows go from 2,916,564 bytes across seven populated maps to
1,638,416 bytes in one (`closure.body_registry`), −44 %, with byte-identical
program output. Projected onto cc's recorded census counts (59,384 distinct
bodies, 58.5k of them strict, 27k arrows, 6.8k dispatch-cache entries) the
same estimator gives 3.28 MB against the 7.24 MB before, −55 %; the
remaining floor is hashbrown's power-of-two bucket count at that size. The
census now prints `closure.body_registry` and `closure.trusted_targets` in
place of the ten per-attribute rows.

Not in this change: the `fn.name_registry` / `fn.source_registry` tables the
issue mentions (5.4 MB on the same census) keep their own `func_ptr` keying —
folding them in wants the dense function-id scheme, which this does not
introduce. `scripts/gc_runtime_root_holders.json` records the two new
statics under the same `not_a_gc_pointer` verdict the deleted maps carried
(code pointers and plain integers only) and drops the eight stale entries.
