### Fixed

- `ns.member(...spread)` on a `const`-bound rest-parameter export (`export const mergeAll = (...ctxs) => …`, e.g. effect's `Context.mergeAll(...contexts)` inside `Layer.mergeAll`) now calls the closure; the spread fast path treated the export's value getter as the function body and evaluated to the function instead (#10197).
