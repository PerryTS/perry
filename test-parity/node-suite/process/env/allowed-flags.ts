// process.allowedNodeEnvironmentFlags — Set of NODE_OPTIONS / V8 flags
// the runtime will honour from the environment. Perry binaries are AOT
// and don't honour runtime flags, so the empty Set is the spec
// shape. Regression cover for #1380 (Perry was returning a 0 sentinel
// so `.has` / `.size` / iteration all threw).
const f = process.allowedNodeEnvironmentFlags;
console.log("instanceof Set:", f instanceof Set);
console.log("size typeof:", typeof f.size);
console.log("has(--bogus):", f.has("--bogus"));
