### Fixed

- Kept non-callable value-call errors catchable across the checked-unbox runtime boundary, refreshed the native-value-profile parity expectation for its intentional POD-copy coverage, and removed a duplicate stdin listener match arm that made current `main` fail the warnings gate.
