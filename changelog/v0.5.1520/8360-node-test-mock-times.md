### fix(node:test): honor finite mock implementations

`mock.fn()` and `mock.method()` now validate and honor the `times` option,
falling back to the original implementation after the configured number of
calls while preserving indexed one-shot overrides. Mocking a missing or
non-callable target method now also reports Node-compatible validation errors.
Advances #6767.
