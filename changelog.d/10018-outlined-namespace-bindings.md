### Fixed

- Preserve module-variable binding origins in dynamic imports and barrel re-exports after large module initialization is outlined into compiler-owned helper functions. Namespace analysis now reconstructs the logical entry statements instead of falling back to undefined-returning barrel getters; dollar-prefixed bindings no longer produce missing getter symbols at link time.
- Add an application-independent native regression for renamed imported variables, live updates, object identity, multi-hop re-exports, and dollar/underscore name collisions, with entry outlining both disabled and forced at O0, Os, and Oz. The forced arm asserts that outlining actually happened, and a Cargo integration entry point runs the matrix in scoped PR CI.
