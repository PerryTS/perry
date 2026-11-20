### Preserve Next App Route async-local state across provider boundaries

`AsyncLocalStorage` now reads and mutates context through a runtime-owned C
ABI when the app, runtime, and standard library are separate shared-library
images. Promise, `async`/`await`, dynamic-import, microtask, timer, and stream
continuations therefore snapshot the same action, request, work, and work-unit
stores that `run()` entered, without cross-request contamination.

Nested `run()` and `exit()` scopes still restore their prior state after normal
completion, throws, and rejected promises. Focused Next-style coverage also
checks two interleaved request IDs, a request after rejection, and an empty
context after route completion.
