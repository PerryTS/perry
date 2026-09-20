**Unary `+` now proves a Number by construction.**

`expr_numeric_by_construction` required `rec(operand)` for `Pos` as though it were a soundness guard. It is not: unary `+` is ToNumber, which either completes holding a Number or throws — BigInt and Symbol throw, an object re-enters ToNumber after ToPrimitive, `undefined` is NaN. A throw stores no value, so the store-universe question the fixpoint asks is vacuous there.

`Neg` and `BitNot` keep their condition, because ToNumeric is BigInt-preserving (`-1n` is `-1n`).

The missed proof left the *accumulator* unproven, so its add kept a per-iteration tag test: `const v = +o.a; … h += v` goes **20 → 9 instructions per iteration**, the same figure `o.a * 1` and `o.a - 0` already reached.
