perf(runtime): `Object.create(P)` results are allocated as wide as their
descendants grow (#10905). The keyless birth shape `(P, [])` now records how
wide the objects born on it grow — the largest key count of any shape minted
below it, raised by any descendant that spills — and a birth is allocated at
that width (8 slots for the first 8 births while it tracks, then exactly the
learned width, capped at 64, and the old two-slot floor when nothing grew).
Before, every `Object.create` result had two inline slots, so its third own
field and every later one lived in overflow storage for the object's whole
life. On the acceptance matrix (release build, instructions per op over the seven
provenances) the `Object.create` column moves from 264-334 to 48-120 on
`overwrite`, 295-369 to 79-151 on `read1`, 710-810 to 234-326 on `read4`,
452-528 to 79-151 on `addkey` and 525-598 to 311-380 on `inherited`; the
literal column is unchanged (its `addkey` cells are 7 lower). The width is
capacity only (keys stay authoritative) and is never consulted by a read or a
write.
