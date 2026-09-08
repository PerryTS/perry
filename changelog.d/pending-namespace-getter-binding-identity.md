Fix namespace live getters for exported variables whose raw local name contains a dollar sign. Preserve that binding's raw getter symbol instead of sanitizing it into a colliding, unrelated function name.

Adds an in-process IR regression selected by scoped PR CI, and a dynamic-import fixture with a Node/native comparison at LLVM O0, Os, and Oz (`scripts/test-namespace-getter-binding-identity.mjs`). The dynamic import exercises the live getter rather than a projected static import. This fix is independent of function export alias handling.
