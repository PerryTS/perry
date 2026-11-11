`export * from`, `export { x } from` and `export * as ns from` kept their
original specifier, so a package aliased through `perry.packageAliases`
resolved to the unaliased package from re-export sources. The alias map is now
applied to every re-export source after lowering.
Aliased imports and re-exports keep matching request metadata, preserving
source-order initialization when aliases converge.
Transitive re-export scanning and export-demand pruning follow the same alias
targets, retaining bindings requested through barrels.
