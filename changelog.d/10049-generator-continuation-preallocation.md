Fix captured local callbacks becoming undefined after `yield*` inside an async
generator loop (#10048). The reused boxed-declaration path now initializes a
missing cell before evaluating its initializer, without confusing an existing
stack slot with an executed box allocation. Existing live cells are retained,
preserving shared hoisted-var bindings. Module globals and preallocated cells
remain on their existing paths; specialized async control cell types are kept.

Adds codegen coverage for initialized and uninitialized boxed declaration
copies, plus independent native parity fixtures for delegated loops, retained
iteration callbacks, empty delegates, ordinary yield/await, recursion, TDZ,
and shared hoisted-var bindings.
