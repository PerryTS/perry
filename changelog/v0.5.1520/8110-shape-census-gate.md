### Fixed

- Wire the `ObjectHeader` shape-descriptor census into `lint`. #8086 built the
  exact-callsite census #8067 required — the instrument that keeps
  `object_type`, `field_count` and `keys_array` retired while `ShapeId` takes
  over their facts — but referenced it from no workflow, so it had never been
  able to fail a build. Wiring it up exposed that its emitted-guard arm was
  vacuous: it rejected `add(..., "0"|"12"|"16")`, while all four functions it
  names build their header address with `blk.gep(I8, &p, &[(I64, "N")])`, so a
  planted read of the removed `keys_array` offset in
  `emit_element_shape_field_load` left the census green. The check now matches
  the gep form, and each of the four emitters is sabotage-verified red.
