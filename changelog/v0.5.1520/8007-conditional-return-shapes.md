### Representation selection: prove fresh conditional return shapes (#7170 R2)

Functions and CJS-wrapped closure producers that return conditional expressions
can now issue a `Ptr<Shape>` return fact when every recursively reachable result
arm is a fresh allocation of the same admissible class. Caller bindings then
reuse the existing guard-free fixed-offset field-access path, with the existing
class-admission, containment, module-barrier, and GC-rooting proofs unchanged.

Non-fresh or disagreeing arms remain fail-closed, as do logical expressions and
locals nested inside conditional arms. `--opt-report` keeps these allocations in
the honest `returned expression operand` syntax bucket while marking only the
allocations consumed by an issued return-shape fact as served.
