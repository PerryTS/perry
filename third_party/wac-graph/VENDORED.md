# wac-graph 0.12.0

This is the crates.io `wac-graph` 0.12.0 archive, SHA-256
`f0418130ea1c8f995ac51fd495080942604bca6455e6fb11f110aa1cefd3f549`.
The upstream Cargo.lock is omitted because Perry resolves the dependency in its
workspace lock. All other upstream files are unchanged except `src/encoding.rs`
and `src/graph.rs`.
Upstream provenance is retained in `.cargo_vcs_info.json`; Cargo.toml declares
`Apache-2.0 WITH LLVM-exception`. The upstream repository license is bundled
separately because crates.io omitted it from the archive.

The encoder's root function types can refer to imported resource instances.
Upstream `borrow` asserts that a nested type scope exists and both `borrow` and
`own` look up only the current scope's resource-name table. That table lacks
resource aliases at the component root. A valid freestanding import with a
borrowed resource therefore panics during component composition.

The local `resource_index` helper preserves existing scoped resource bindings.
At the root it reuses resource type indices or aliases the matching imported
instance's resource export by its resolved nominal ResourceId. The graph records
resource exports by identity when importing an instance, so anonymous interfaces
and different resources with identical names do not depend on interface names.
Nested function signatures can also alias a resource from an outer scope even
when their anonymous interface does not re-export that type. Direct resource
imports are cached by nominal identity. Borrow and own use the same resolver.
Perry's C-boundary interop tests exercise the actual composed components.

Shared value-type aliases also bind by the concrete `InterfaceId`, rather than
the canonical interface-name string. Two labeled imports may both implement
one canonical interface while defining different nominal resources. Register
both instances for dependency/value/resource alias lookup, and reuse only the
same concrete dependency identity. Perry's composition regression checks the
exact public import set, preserves the selected record's resource identity,
and rejects a facade that uses the other labeled instance's owner type.

This is a source fix to the pinned release, with no dependency version/soak
exception. Remove the vendor after the fix is available in an upstream release
that passes Perry's normal dependency soak and compatibility checks.
