Canonical i32 representation selection can now admit a loop accumulator when
the compiler can prove that its initial magnitude plus every loop trip-count
bound times every step-magnitude bound stays within signed 32-bit range.

The proof derives saturating trip counts from constant-bounded `for` induction
variables, multiplies them through nested loops, and starts with a deliberately
small step-expression set: integer literals and constants, remainder by a
literal, bitwise-and with a non-negative literal mask, and another loop-bounded
local. Unknown loop counts, writes, and expression forms remain boxed. In
particular, the factorial-style `sum += i % 1000` counterexample is still
refused because its bound exceeds i32.

The change includes a representation-census liveness promotion and a registered
runtime parity probe whose three one-billion additions make an unsound widening
print a wrapped value instead of Node's 3,000,000,000.
