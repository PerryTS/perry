### Property reads: one receiver compare on every route, and site caches hold only shape facts

**Fused receiver test (S4).** Every inline property route now tests its receiver
with one unsigned range compare, `bits - (POINTER_TAG | 0x10_0000) <u 2^48 -
0x10_0000`, which is exactly "POINTER tag and a payload above the native-handle
band". Before, it was a tag test plus a small-handle test. #11161 introduced the
compare for the class-field read guard. It now lives in
`perry-codegen/src/expr/receiver_range.rs` and is used by the generic read IC,
the read-region guard, the class-field read and write guards, the class-field
loop preheader, the `"k" in o` presence site and the cached field-index return.
Loads on the pointer edge address off the biased value, so the handle is never
materialised on a hit. A unit test checks all 65,536 tags against every
payload boundary. Measured, in instructions per iteration: generic read
`realsite` 112 → 109, ops4 `p1`/`p2` 47 → 43, `p2m` 46 → 41, `wget` 185 → 181,
`monoext` 159 → 156. `ts.transpileModule`: −1.18% instructions (−2.09 G), output
identical.

`PERRY_RECV_ROUTE_COUNT=1` at compile time counts executions per route (census
builds only; product builds emit nothing).

**Site state derived from one shape only (S6).**
- *Dictionary shapes are unmatchable.* Dictionary-mode ShapeIds are minted in
  their own band, `[0xB000_0000, 0xC000_0000)`, chosen by the shape's
  generation namespace. Every site-word writer admits only the ordinary band:
  the compact word, the ways, region words, the read and write stubs, and
  presence sites. A dictionary receiver keeps its id across appends and some
  value-moving deletes, and on main the global read stub then answered
  `o.x` with `y`'s value (`test_gap_dictionary_receiver_site_memo.ts`).
- *The Array-subclass named-prefix token is gone from cache word 2.* It remains
  on the object as a prime-time proof only. A descriptor-bearing subclass
  instance still primes its `(ShapeId, slot)`.
- *The poisonable `@perry_class_guard_shape_*` twin is gone.* Class-field guards
  compare against the class's own `@perry_class_shape_id_*`. A declared field is
  an own data property from birth, so a prototype accessor never changed the
  answer.
