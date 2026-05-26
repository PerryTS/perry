// Helper for test_gap_namespace_member_not_array_method.ts (#321 / #24).
// A module that happens to export functions named like array methods
// (`map`, `filter`, ...) — exactly effect's `export const map = core.map`
// shape — plus a real array export.

export const map = (x: number, f: (n: number) => number): number => f(x);
export const filter = (x: number, p: (n: number) => boolean): boolean => p(x);
export const find = (x: number): number => x + 1;

// A real exported array, to confirm `NS.items.map(cb)` still array-maps.
export const items = [10, 20, 30];
