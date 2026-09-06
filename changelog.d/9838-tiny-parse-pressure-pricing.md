**The tiny-parse pressure guard now prices the collections it forces by the
adaptive step's productivity backoff (#9831).** On the compiled claude-code
TUI a 3300-character streamed reply spent 30–41 s of CPU in the base arm and
27.8–29.2 s with the fix (mean −19 %, every interleaved pair a win), with
post-turn and post-idle RSS flat within the base's own spread and peak RSS
unchanged.

#9831 measured the `ArenaBytes` arm firing 51 times in one 66-delta reply,
each collection freeing a median 131 KB, while the adaptive step sat
saturated at 1 GiB — and located the discarded backoff in the arm's own
ceiling clamp. That clamp was not what re-fired the arm: between two firings
the arena grew a few hundred KB against a trigger armed 16–128 MB above the
post-collection total. What pulled the trigger down was the tiny-parse
pressure guard, which after every `JSON.parse` growing the arena by ≤ 1 MB
tested the absolute `arena_in_use_bytes() >= 48 MB` and, if it held, set the
trigger to "now". That is a quantity no collection can lower below the live
set, so on a heap that sits above it permanently every small parse (one per
SSE delta) forced a minor whose backoff nothing read — #9589's shape one
trigger over.

The guard now also requires the arena to have grown, since the last
collection of any kind ended, by a headroom priced from the step: the step
rescaled so its power-on value buys the 16 MB headroom floor and each
doubling the arm's clamp discards buys one more doubling, bounded by the
trigger ceiling. A productive collection keeps today's cadence; an
unproductive one earns room. The parse-boundary collector re-prices a
pending request so a collection that already satisfied it is not followed by
a second. `PERRY_GC_DIAG=1` gains a `[gc-tiny-parse] forced collection …`
witness line. The arm's own arithmetic is unchanged and now documents why
(pricing it directly was measured at −10.8 % CPU for +22 % footprint, the
issue's refuted branch).

Validation: `test_memory_json_churn.ts` (the guard's motivating shape) is
byte-identical in output and RSS in all four GC modes; 48/48 `test_gap_gc_*`
and 8/8 `test_gap_json_*` pass; nine new `gc::tests::tiny_parse_pressure`
tests pin the pricing and the predicate, sabotage-proved against both the
old absolute guard and a raw-step pricing.
