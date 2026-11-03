### Fixed

- Aggregate scalar replacement now keeps a fixed object-array carrier materialized when an arrow-function body reads it, preventing module-scope arrays from disappearing underneath closures.
