The dynamic write IC does one shape-descriptor lookup per hit instead of two.

`object_shape_id` runs a full shape-table lookup — and copies the whole
`ShapeDescriptor` out of the table — purely to prove the stamped id is live,
then discards it. `dyn_ic_try_store` called it for the token compare and then
looked the SAME id up again for the slot bound; both write re-prime sites did
the same while already holding a descriptor.

It now reads the stamp off the header. In `dyn_ic_try_store` the token compare
runs first, so a wrong-shape receiver costs a load and a compare with no table
work at all, and the single lookup that supplies the slot bound doubles as the
liveness proof: a stamp with no live descriptor returns `None` exactly where
`object_shape_id`'s 0 made the token compare fail before.

Verified by profile rather than by wall clock, because the effect is close to
the noise floor of the benchmark. In a computed-key write loop
`shape_descriptor_by_id` falls **7.01% → 3.49%** of self time and
`object_shape_id` (1.32%) leaves the profile entirely.

Wall clock, interleaved A/B min-of-21 at quiet load, against the exact parent
commit: write-only loop 42 → 40 ms min (−4.8%), mean 46 → 45. The combined
overwrite loop and the read loop are unchanged, as expected for a change
confined to the write IC. Computed-key differential output is byte-identical
to node; suite 2779 passed, 0 failed.
