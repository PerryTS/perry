Fix captured local callbacks becoming undefined after `yield*` inside an async
generator loop (#10048). Each emitted preallocation directive now initializes
its own control-flow path, reusing an existing local slot without confusing
emitted storage with an executed initialization. Scope re-entry gets a fresh
cell while retained closures keep their original cells. Module-global
precedence, TDZ initialization, and specialized async control cells are kept.

Adds fail-before/pass-after codegen coverage for ordinary and TDZ continuation
copies, plus independent native parity fixtures for delegated loops, retained
iteration callbacks, empty delegates, ordinary yield/await, recursion, TDZ,
and shared hoisted-var bindings.
