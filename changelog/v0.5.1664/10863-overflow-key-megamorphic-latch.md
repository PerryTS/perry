Let a property-read site whose hot key lives in the overflow (spill) region
reach the megamorphic latch. `pic_prime_get` used one predicate to answer two
questions — whether the displaced MRU entry may be absorbed into a polymorphic
way, and whether a different shape displaced it at all — and one `return`
served both, so the consecutive-eviction run never advanced on such a site and
`PIC_WAY_STATE` never went negative. The site kept the armed state it earned
during warm-up for the life of the process, and the emitted gate ran four
dependent loads and a compare tree on every read that could only ever miss.

Split the two decisions. A cascade suppressed because the displaced slot is
overflow-encoded now advances the eviction run on an armed site and latches at
the same threshold an inline rotation latches on. Nothing is written to a way
on that path: keeping overflow-encoded slots out of the ways is what makes the
emitted way path's raw `obj + header + slot * 8` address computation safe, and
that is unchanged. A rotation in which no shape carries the key inline never
arms in the first place and is not affected.

Measured on a 64-shape rotation with the hot key in overflow: primes taken
while the site was armed fall from 100.0 % to 0.76 %, megamorphic rises from
0.0 % to 97.0 %, and the read costs 20.3 fewer instructions (-2.7 %). A site
rotating the same shapes with the key at an inline slot is unchanged to two
decimal places, as are the `own3`, `w4`, `inh` and `inh3` fixtures.

`PERRY_IC_DIAG` gains a `way_encoded_slot` counter, which scans the ways for a
slot word carrying the overflow bit on every prime and must always read zero.
