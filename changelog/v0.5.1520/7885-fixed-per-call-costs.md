### Three fixed per-call costs on the string-builder, generic-equality and write-barrier paths

**1. `js_string_concat_chain` initialised ~2 KB of stack on every call, whatever the
chain length.** The helper sized its scratch at the codegen cap of 32 parts, so a
two-part chain paid the same `memset(_, _, 0x400)` for `num_bufs` plus 32 `str xzr` for
the handle array as a 32-part one. Confirmed in the shipped release disassembly, not
inferred: `sub sp, sp, #0x7e0` / `bl _memset` with `w1 = #0x400`. Real chains are 2-4
parts — `seen = seen + "[" + names[i] + "]"` in an environment lookup is four, and the
codegen fold (which does fire; that was checked) turns it into exactly one call per
append. The body is now monomorphised on the scratch size and dispatched `n<=4` /
`n<=8` / `<=32`, with `num_bufs` left `MaybeUninit` so only slots a numeric arm formats
into are written.

**2. A strict `===` whose operands are both statically unconstrained emitted one
`js_eq` call and nothing else.** The shape that pays for this is a linear scan over a
generic container's key array (`this.keys[i] === k` in a `Registry<K, V>`), which is
dominated by *misses* — and a miss reaches `js_jsvalue_equals`'s pointer arm, which runs
`resolve_forwarding` twice. An inline prefix now settles four cases without a call:
identical bits (excluding a plain IEEE NaN), two SSO strings with different bits, two
INT32s with different bits, and two `POINTER_TAG` values with distinct in-band addresses
whose `GcHeader`s do not carry `GC_FLAG_FORWARDED`. The last is an exact restatement of
`resolve_forwarding`'s "neither is forwarded, so fall through to 0", behind the same
magnitude guard the runtime applies before any header dereference.

**3. The opaque write-barrier wrapper had no value test.** `write_barrier_slot_inner`'s
first action is `barrier_child_prologue(child)?`, so `js_write_barrier` does nothing at
all for a non-pointer child — yet an array element store on a `number[]` emitted a bare,
unconditional call next to every element write. #7511 put exactly this gate on the
class-field slot store; `emit_write_barrier` never got it. It now goes behind
`emit_may_carry_heap_pointer_check`, which is a deliberate superset of the runtime
predicate (`gc::tests::inline_pointer_bearing_contract` enumerates the whole 16-bit tag
space against it), so it can only skip calls that would have returned immediately.

Two probes were added with the work: `gc-handoff/bench/strbuild.ts` (10M four-part
concat chains) and `gc-handoff/bench/eqscan.ts` (2.4M generic-container key scans).

Measured on the quiet M1 mini, best-of-5, interleaved, exit-checked, `VERDICT: CLEAN`
(load 1.79 → 2.04, zero foreign processes at both ends). Absolute seconds:

| bench | before | after | |
|---|--:|--:|--:|
| `strbuild` (concat probe) | 0.7448 | 0.5477 | −26.5% |
| `eqscan` (key-scan probe) | 0.2181 | 0.1720 | −21.1% |
| `iso_miss` | 1.2319 | 1.1283 | −8.4% |
| `pipeline_big` | 2.5334 | 2.3291 | −8.1% |
| `pipeline` | 0.2646 | 0.2444 | −7.6% |
| the other 18 corpus programs | — | — | 0.978 – 1.005 |

The 15 programs whose emitted IR contains no `js_string_concat_chain` call site and whose
binaries are byte-identical across the two codegen arms set the run's noise floor at
±0.5–2%; every mover is outside it.
