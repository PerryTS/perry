### `Promise.all`: a guarded per-element fast arm, and a measured "no" to routing it natively (#7911)

`Promise.all([...])` lowers to `js_promise_all_iterable` → the spec combinator
(`spec_combinators.rs::run_combinator`). #7911 asked whether it could instead be
routed to the simplified native `js_promise_all`, whose fan-out is ~29 % of the
`asyncpipe` benchmark — and deliberately framed that as an open question rather
than a given.

**The answer is no, and it is measured rather than argued.** Seven probe files
(~90 assertions) were written against `node 26.5.1 --experimental-strip-types`
first, then run against the current path and against a throwaway build that
routes `Promise.all` into `js_promise_all`:

* the current spec path matches node on **7/7** `Promise.resolve` observables
  (read once off `C`, invoked per element with `this === Promise`, a patched
  `resolve` transforming the values, non-callable → `TypeError`, a throwing
  `resolve` propagating, `Promise.resolve(p) === p`). Routed natively: **0/7** —
  `Promise.resolve` is never read at all.
* an own `then` on an element is invoked today and ignored when routed.
* **the routed build dies**: `js_promise_all` never invokes `then`, so it never
  runs `mark_rejection_handled`; a rejection the combinator swallows stays
  flagged unhandled and terminates the process. Two of the seven probes exit 1
  with `Uncaught (in promise)`. `Promise.all([Promise.reject(a),
  Promise.reject(b)])` inside a `try/catch` is an ordinary shape.

The cost, however, is not intrinsic to the spec algorithm — it is in HOW the two
observable per-element steps run. `Call(promiseResolve, C, «next»)` reaches
`js_promise_resolved(next)` through a setjmp frame, two `js_implicit_this_set`
TLS writes, a closure dispatch, a `RuntimeHandleScope`, and an
`is_default_promise_constructor` that allocates a fresh `"Promise"` key string
per element. `Invoke(nextPromise, "then", …)` reaches `js_promise_then` through
a second setjmp frame, the whole `native_call_method` tower and three
own-property probes, and then allocates a chained promise per element that
`Promise.all` immediately discards.

So this adds a per-element fast arm that calls the primitives those towers
select, entered only when: `C` is the intrinsic `%Promise%`; the
`promiseResolve` value — already read once, observably, by `get_promise_resolve`
— is the reified `promise_resolve_static` thunk (matched by closure FUNCTION
POINTER, because `Get(Promise, "resolve")` may reify a fresh closure object per
read); neither `v8.promiseHooks` nor `async_hooks` is active; and the value the
resolve step produced is a genuine `GC_TYPE_PROMISE` with no own `then` and no
own `constructor`. The last two are exactly the tests
`native_call_method/primitive_methods.rs` makes before jumping to
`js_promise_then`, and they are re-tested on the RESOLVED value rather than the
raw element because `js_promise_resolved` returns the element itself for promise
identity. The hooks clause is what makes dropping the discarded chained promise
unobservable — `js_promise_attach_handlers` is `js_promise_then` minus that
allocation, and the allocation's only external effect is the lifecycle
callbacks.

Nothing the spec makes observable moves: the iterator drain,
`NewPromiseCapability`, the single `Get(C, "resolve")`, the resolve-element
closures with their `[[AlreadyCalled]]` guards and `remainingElementsCount`, and
the final `Call(cap.resolve, …)` are all unchanged. Only a boolean is hoisted
out of the loop — the capability's reject function is a GC object, and caching
its raw address across a loop that allocates a guard array and a closure every
iteration is the #7184/#7497 defect shape verbatim, so that address is re-read
from its handle at each use.

**Measured** (instructions retired, `/usr/bin/time -l`, both arms from one
binary; no wall clock quoted — the dev box ran at load 17–90):

| program | before | after | Δ |
|---|--:|--:|--:|
| 1200×200 fan-out, settled elements | 548 345 659 | 397 709 920 | −27.5 % |
| 1200×200 fan-out, pending elements | 803 142 312 | 651 573 199 | −18.9 % |
| `asyncpipe` @240 / @360 / @480 batches | — | — | −33.9 % / −34.3 % / −30.7 % |
| `asyncpipe` @120, nursery cap inactive | 1 686 481 864 | 1 534 500 019 | −9.0 % |
| `asyncpipe` @120, default config | 1 927 275 215 | 2 017 785 565 | **+4.7 %** |

`PERRY_MT_PROFILE=1` on `asyncpipe`: `then=23800 → 0`, `new=95641 → 71841`; every
other counter unchanged.

★ **The one regression is a GC trigger boundary and is reported, not hidden.**
`asyncpipe` at its shipped 120 batches runs 0 copying minors before and 1 after
(`copied_objects=156236`, `trigger=ArenaBytes`); arena in-use at the decision
point is 17 452 448 B vs 17 896 616 B, a 444 KB window on 17.5 MB. At 240/360/480
batches, where both arms run the same number of copying minors, the change is
−31 %…−34 %. This is the knife edge `gc-handoff/ASYNC2-NOTES.md` already
documents, and the consequence for future work is that **`asyncpipe`@120 is not
a usable A/B target for `Promise.all` changes** — the scaled variants are.

Validation: 19/19 corpus programs byte-exact and identical between arms; 98
async/promise/stream/timer/thread `test-files` with zero arm-vs-arm differences;
the seven probe files byte-identical to the pre-change build, including all
eight pre-existing node divergences the probes surfaced (patched
`Array.prototype[@@iterator]`, patched `Promise.prototype.then`, spec iteration
interleaving, `class Q extends Promise` with an explicit constructor, an
element's own `constructor` — all documented in
`gc-handoff/PROMISEALL-NOTES.md` for separate issues). Two new gap files pin the
observables on both sides of the guard, and six `perry-runtime` unit tests cover
the guard predicates — one of them asserting via a `cfg(test)` tally that the
fast arm was actually TAKEN, so a fast path that silently stopped firing fails
rather than passes. No env knob ships.
