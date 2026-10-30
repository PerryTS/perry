`export * from`, `export { x } from` and `export * as ns from` kept their
original specifier, so a package aliased through `perry.packageAliases`
resolved to the unaliased package from re-export sources. The alias map is now
applied to every re-export source after lowering.
