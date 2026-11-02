### Fixed

- CommonJS cycles no longer emit missing-property warnings at `require` time for reads inside non-immediately-invoked functions. This removes spurious warnings from iovalkey while retaining warnings for missing properties read directly during a cycle.
