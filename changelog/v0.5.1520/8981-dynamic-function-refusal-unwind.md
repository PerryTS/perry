### Fixed

- Refusing a runtime-string `Function` in a `dyn-eval`-off AOT binary now throws a catchable `TypeError` instead of aborting at the runtime FFI boundary, allowing zod v4 and other capability-probing libraries to select their non-eval fallback.
