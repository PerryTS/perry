### Fixed

CommonJS modules that define their complete export with
`Object.defineProperty(module, "exports", descriptor)` are now detected and
wrapped correctly. Packages such as `abstract-logging` no longer see
`module is not defined` or an empty export object.
