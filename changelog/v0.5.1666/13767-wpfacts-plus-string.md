## wpfacts: `+` with a String operand is a String; a guard on a marker beside its values waits

`+` now follows ECMA-262 13.15.3: after ToPrimitive a String operand makes the result a String, and
the numeric kinds of one operand reach the result only once the other operand may be no String
(`Cons::Plus` per operand, a `Guard` with the new test `NOT_STR`). Before, each operand added its own
kinds, so `"" + x` with a host or `any` `x` was String|Number|BigInt and every member read on a String
accumulator (`this.input += key`, `line += word`) stayed unproven. `Cons::Arith` is the ToNumeric
operand only. A `Guard` on a marker beside its values (a described parameter, a published method's
receiver and their reach markers) no longer admits the test by itself: it waits for the values and
fires if the marker leaves, as arithmetic already did.
