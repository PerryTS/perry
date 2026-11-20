### Fixed

- `export * as Self from "./self"` now reads as the module's own namespace object through a dynamic `import()` namespace (and its destructuring); the entry was `undefined` because the populator loaded the module's namespace global before it was stored (#10160).
