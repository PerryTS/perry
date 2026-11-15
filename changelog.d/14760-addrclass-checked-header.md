The at-rest Array layout accessor (`layout_slot`) reads an Array's GC header
through the array module's one header probe instead of a duplicated copy with
its own address floor. The instrumented lifetime census (`PERRY_LIFE_ALLOC`,
`PERRY_LIFE_MEASURE`, `PERRY_LIFE_REGIONS`, instr builds only) classifies a
birth or region address with the handle-band-aware header read before
dereferencing it, so a handle-band id that carries POINTER_TAG is counted as
unborn or not alive instead of being read as a header.
