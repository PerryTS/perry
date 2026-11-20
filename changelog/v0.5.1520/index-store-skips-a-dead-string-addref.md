An indexed array store no longer emits `js_string_addref_if_heap_string` for a
value that provably cannot be a heap string.

The plain slot-store emitters pass `layout_note_needed` for *both* the layout
note and the string-addref demote, so any store that needs a layout note also
paid the addref call — including `sieve[j] = false`, where the value is a
boolean. The flagged emitter that separates the two already existed and
`array_push` already used it; this adds the scalar-aware twin and threads
`store_needs_string_addref` (the same predicate the push path trusts) from the
one caller that has the value expression in hand.

`benchmarks/suite/11_prime_sieve.ts` goes from 8 emitted addref calls to 1.

Measured on an idle Mac mini, both binaries built in one run, interleaved, min of
five, self-timed: a boolean-store loop 215 → 207 ms, and `11_prime_sieve` 28 → 26
ms. Roughly 4% each — small, and worth saying plainly that it is not where either
benchmark's gap lives: the same measurement puts Node at 12 ms on that
boolean-store loop against perry's 207, so the per-store typed-feedback guard
**call** is the cost that matters there. This change removes provably dead work
beside it.

Verified against Node on a differential aimed at the exact hazard the addref
exists to prevent — a refcount-1 string stored into a slot and then mutated
through the source local (directly, and through a loop-written array), booleans /
numbers / `null` / `undefined` stored into an array, the sieve shape itself, and a
string slot overwritten by a boolean and back. Byte-identical. 31 `perry-codegen`
suites pass.
