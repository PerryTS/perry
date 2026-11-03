Read regions over a `+` tree are guarded **once** (#10884 step 4b, slice 1).

A single-entry run of accesses over which one receiver's `(pointer, ShapeId)`
pair is held, entered through one shape compare whose failure leaves for a
generic copy and never rejoins:

```
[R1] guard   tag test + unmask + ONE ShapeId compare
[R2] load    every key's slot, from one atomic region word
[R3] verify  every leaf is a primitive Number
[R4] use     fold the tree with fadd
```

The region does not compute the wrong answer when an operand is unfriendly — it
declines *before* computing one. Hoisting every leaf above the additions is
exactly what #10904 got wrong; it is legal here because R3 proves no addition
can reach `ToPrimitive`, and a failed check discards the loaded values and
lowers the tree afresh in source order in the generic copy. That re-evaluation
is only sound because every admitted leaf is effect-free: a read of the guarded
receiver, a local, or a numeric literal.

The expected id and every key's slot live in one atomic word, and every refusal
path yields the **empty** word — a wrongly packed slot is a wrong value, while
an empty word is only a missed fast path. `region_guard_pack_tests` pins that,
including that the empty word can never match a live receiver because its low
half is not a ShapeId.
