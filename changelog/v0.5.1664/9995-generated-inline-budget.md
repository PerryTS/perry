### Fixed

- Cap force_inline admission at 8 KiB of estimated generated IR so minified single-expression functions cannot force megabytes into each caller. Preserve the separate pre-statepoint budget and small helpers.
