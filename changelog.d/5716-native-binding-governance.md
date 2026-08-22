Defined an enforceable policy for bundled native bindings: ordinary
JavaScript and TypeScript packages now have recorded upstream-source migration
targets, native/domain integrations have external-package targets, and shared
runtime APIs have explicit retain/consolidate decisions. Release builds consume
the governed inventory. The first completed migration removes both native
`slugify` implementations and compiles installed `slugify@1.6.9` source through
default package routing, with a Node-parity E2E covering its full option and
extension behavior.
