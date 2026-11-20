**GC: the nursery constant band is denominated in objects, not bytes (#7929).**

The scavenge nursery trigger compared from-space *bytes* against a constant
16 MB band while the copying minor's cost is per *object*, so shrinking a
representation silently bought the collector more work per cycle: #7928 took a
two-field object literal 72 B → 56 B and every minor then moved **1.286×
(= 72/56)** as many objects for the same bytes, costing `deeplist` +10.5% and
`retain1` +8.8%. The band is now scaled by the mean size of the objects the last
copying minor actually moved, so it buys a constant object budget. The mean comes
from the census the collector already produces — nothing is added to the
allocation fast path, and the arena still needs no nursery object counter.

Only the constant band is re-denominated. The two `tenuring.rs` ratios
(`desired_survivor_bytes` = cap/16, `retune_nursery_cap_scale`'s cap/25 and
cap/100 bands) and the tenured-proportional arm of the cap are
representation-invariant by cancellation — `tenured_bytes / 2` is
`tenured_objects / 2` objects — so scaling them too would double-count.

The scaling is **one-sided** (clamped at 1.0): a mean above the 72 B calibration
reference keeps today's band. That neutralises an array-dominated mean —
`push_num` measures 3600 B because it survives arrays, the documented blocker on
the issue — and leaves every program at or above the reference bit-identical,
because *raising* a cap is the direction that pushes a program across
`GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES` or into a #7909 budgeted stall.

Measured on the shipped 19-program corpus against `origin/main`, all 19
byte-exact vs `node --experimental-strip-types` 26.5.1 and exit 0 (also under
`PERRY_GC_PROTECT_FROMSPACE=1` + `PERRY_GC_VERIFY_EVACUATION=1`), instructions
retired: `deeplist` −72.3% / RSS −27.2%, `retain` −62.9% / RSS −26.4%, `retain1`
−11.3% / RSS −6.5%, `churn` family −1.0%. The five programs whose factor is
exactly 1.000 form a built-in control set; their −0.3%…−3.5% spread is this A/B's
link-layout noise floor, and `interp` (+0.2%) and `iso_miss` (+0.4%) sit inside
it. `retain1` is the clean cell — identical cycle kinds in both arms, −11.8%
object work for −11.3% instructions, against the +12.0% #7928 cost it.

The blocker recorded on the issue — "an object term fires the collector earlier
and every extra cycle pays a fixed root scan" — is refuted by measurement: an
extra copying minor costs **~0.5 M instructions** (consistent with #7915's 218 455
pointer roots) while an object moved costs **~1050**, so an extra cycle pays for
itself if it avoids ≥ ~500 object-moves. On the taxed programs the trade is
~500:1 in favour, and on `cycles` and `pipeline` more cycles is outright cheaper.

New coverage: a pure-function test that the band buys a representation-invariant
*object budget* (with its own inline sabotage arm, so it declares itself
meaningless rather than passing quietly if the un-denominated band ever stops
differing), one-sided-clamp endpoints named after the failures they prevent, a
carry-forward test for cycles that moved nothing, and an end-to-end test driving
a real copying minor whose discriminating assertion is that the recorded mean
equals *that cycle's* measured mean and differs from the seed — a "mean is
nonzero" check would be satisfied by the seed itself. Both sabotage arms (delete
the `copying.rs` call site; delete the factor from the band) were run and fail
the intended tests.
