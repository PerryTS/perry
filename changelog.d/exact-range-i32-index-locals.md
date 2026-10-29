An integer local takes an i32 slot only when the exact value of every write is
proven to fit int32: `+`, `-` and `*` by interval arithmetic over the operands'
known ranges, and a `for` loop's own step by the guard that bounds it (or the
overflow-exit lowering below). A sum or product of unbounded int32 values, an
unguarded `++`/`--` and `x >>> 0` keep the boxed double, so loop guards,
indices and reads see the number the program computed:
`for (let j = i * i; j <= n; j += i)` with `i = 46349` and `n = 50000` runs
zero times, as in node.

Native typed-array and buffer element accesses check the lowered index against
the length on every access instead of turning the bounds proof into an
assumption. A store outside `[0, length)` does nothing, as in JS; a proven read
that fails the check stops with a named internal error. An index built only
from literals and wrapping operators is checked through its static window, and
a fixed-length view of constant length through that constant, which keeps the
check loop-invariant where it can be. The pixel-channel reduction checks its gathered offsets and
falls back to the scalar statements.

A `for` counter whose init or step is not provably int32 but can only leave
the range upward, past a `<`/`<=` guard with an int32 bound (`j = i * i;
j <= n; j += i`, or `i <= n; i++`), stays an i32: its init is computed exactly
and its step with an overflow check, and an overflow leaves the loop, which is
what the guard does with the exact value. Such loops are versioned on one
entry check of the actual counter, step, bound and lengths: when the step
cannot overflow and every index the guard admits is inside each view the
counter indexes, a copy with plain steps and no per-access bounds checks runs;
otherwise the checked copy does.

Loop clones that validate their own int32 bound (the dynamic-bound loop) admit
a counter outside `integer_locals` with a loop-private i32 copy, and the
versioned indexed loop accepts a canonical int32 bound.
