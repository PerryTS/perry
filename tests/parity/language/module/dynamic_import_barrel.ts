// Helper for test_gap_dynamic_import_reexport.ts — re-exports every name
// from `dynamic_import_inner.ts`. `flatten_exports` should resolve `v` to
// inner's local binding so the barrel namespace exposes `v = 7`.
export * from "./dynamic_import_inner.ts";
