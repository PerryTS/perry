### Representation selection: prove fresh logical return shapes (#7170 R2)

Functions and CJS-wrapped closure producers that return `&&` / `||`
expressions can now issue a `Ptr<Shape>` return fact when every value that can
escape the complete short-circuit expression is a fresh allocation of the same
admissible class. This includes nested fallback forms such as
`(flag && new C()) || new C()`; caller bindings reuse the existing guard-free
fixed-offset field-access path with no new pointer position or ABI change.

Primitive or unknown escape paths, disagreeing reachable classes, and nullish
coalescing remain fail-closed. `--opt-report` marks only allocations that can
actually become the logical result as served, leaving consumed short-circuit
operands out of that population.
