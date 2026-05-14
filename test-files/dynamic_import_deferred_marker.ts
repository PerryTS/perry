// Helper for test_gap_dynamic_import_deferred.ts. Top-level
// `console.log` runs as a side effect of this module's `__init` —
// observable evidence of whether init fired.
console.log("deferred-init-ran");

export const x: number = 42;
