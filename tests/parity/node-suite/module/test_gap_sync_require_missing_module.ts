// A computed CommonJS `require(expr)` of a missing module throws
// MODULE_NOT_FOUND synchronously at the call, so the require's own
// try/catch receives it; an unrelated catch still receives its own error.
import loader from "./node_modules/sync-require-cases/loader.cjs";

console.log(loader.load("missing.cjs"));
console.log(loader.load("missing"));
console.log(loader.probe({ k: 1 }));
console.log(loader.probe(null));
