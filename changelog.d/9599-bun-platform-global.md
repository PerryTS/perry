# Bun platform global namespace

`perry compile --platform bun` now installs a real, stable
`globalThis.Bun` object. Bare `Bun`, direct calls, extracted and destructured
methods, optional chaining, and computed property access share the same native
module registry. The namespace also exposes Perry-defined `version` and
`isStandaloneExecutable` metadata.

The default `--platform node` behavior is unchanged: `typeof Bun` remains
`"undefined"`, preserving Node/Bun feature detection in existing bundles.
