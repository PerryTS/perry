**fix(compile): isolate auto-optimized shared-Tokio wrapper graphs (#9577)**

Parallel `perry compile` processes no longer overwrite one another's
auto-optimized runtime and stdlib archives when they select different native
extension wrappers with the same stdlib feature set. The selected
shared-Tokio wrapper set is now part of the `perry-auto-<hash>` target
directory identity, so each Cargo dependency graph keeps a coherent stdlib
and wrapper archive pair through the final link.

Wrapper aliases that resolve to the same archive, such as `mysql2` and
`mysql2/promise`, are deduplicated before constructing the Cargo package set
and link line. The cache identity remains stable across alias multiplicity and
discovery order.

This removes the temporary compile-smoke exemptions for the Axios and mysql2
fixtures. Those parallel builds now keep distinct target directories while
each wrapper retains the same Tokio compilation ID as its colocated stdlib.
