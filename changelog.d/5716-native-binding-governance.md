Defined an enforceable policy for bundled native bindings: ordinary
JavaScript and TypeScript packages now have recorded upstream-source migration
targets, native/domain integrations have external-package targets, and shared
runtime APIs have explicit retain/consolidate decisions. Release builds consume
the governed inventory, while existing compatibility shims remain bundled until
their documented migration gates pass.
