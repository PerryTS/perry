`assert`'s `AssertionError` is `instanceof Error` again, both when thrown by `assert.strictEqual` and when constructed directly. node-suite `assert` goes from 68/71 back to 71/71.

Bisected to 4e6f09ed6 ("follow builtin prototype chains in instanceof"). For ordinary objects tested against a builtin such as `Error`, that change walks the prototype chain and returned its answer as final, including a miss. `AssertionError` instances have no materialized chain up to `Error.prototype`, so the walk missed and `instanceof Error` became `false` without consulting the class-id chain that knew the subclass edge. Now only a hit from the walk is final; a miss falls through to the class-id chain. `Object.create(Error.prototype) instanceof Error` (#11256) keeps working.

Validation: new `test-files/test_gap_assertion_error_instanceof_error.ts` fails before the fix and matches Node after it; the #11256 gap test still matches Node; `cargo test -p perry-runtime instanceof` passes (15/15); node-suite assert 71/71, buffer/console/events/util 100%, globals and object unchanged.

Known remaining gap, not addressed here: `e instanceof assert.AssertionError` still throws "Right-hand side of 'instanceof' is not an object".
