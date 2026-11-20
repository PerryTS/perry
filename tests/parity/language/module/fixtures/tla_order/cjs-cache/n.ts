import Module, { createRequire } from "node:module";
const require = createRequire(import.meta.url);
console.log("cache", require.cache === Module._cache);
