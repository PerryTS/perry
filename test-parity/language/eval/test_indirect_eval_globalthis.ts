// #1679 (Phase 1 of #1677) — the indirect-eval `globalThis` idiom
// `(0, eval)("this")` folds to the global object (it runs in global scope).
const a = (0, eval)("this");
console.log("indirect_this_type:", typeof a);

const b = (0, eval)("globalThis");
console.log("indirect_globalthis_type:", typeof b);

console.log("indirect_same:", a === b);
