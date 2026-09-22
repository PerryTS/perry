A prototype divergence now mints a **deterministic** generation keyed on a
stable prototype serial (#10868 lever iv).

Two receivers that diverge the same way from the same predecessor previously
minted two generations, and so two ShapeIds, for what is one shape. The
generation is now derived from `(predecessor, prototype serial, link kind)`, so
the same divergence merges and only genuinely different divergences fork.

The key is the prototype's **identity**, not its state, and not its address.
`ObjectMeta` gains `proto_serial`, assigned once by `mark_object_as_prototype`
and never changed: an address moves under the collector (and keying on one is
an address-keyed derived structure), while a ShapeId is shared by distinct
prototypes — unsound, since §3 needs the receiver's shape to determine its
prototype — and changes whenever the prototype gains a key, so two receivers
diverging to the same prototype before and after that would fork.

Every deterministic generation sets bit 63 so it can never alias a
counter-allocated one, and a missing predecessor or serial declines to the
always-correct unique-generation path. `prototype_generation_tests` pins both
halves, per §17's rule that a check which cannot fire is not a check: distinct
prototypes must not collapse (across 4096 serials, not just two), and the same
divergence from the same predecessor must merge — otherwise the lever removes
none of the 48,197 mints it exists to remove.
