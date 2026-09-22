### Fixed

- CommonJS cycles no longer emit missing-property warnings for reads inside functions that run after module initialization. This removes spurious warnings from iovalkey while retaining warnings for missing properties read during a cycle.
