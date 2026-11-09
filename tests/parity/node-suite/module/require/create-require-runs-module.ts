// `createRequire(import.meta.url)` returns a CommonJS `require` relative to
// this module: a literal relative specifier runs the compiled CommonJS file
// and returns its `module.exports`, whose functions then run.
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const loader = require("./fixtures/create-require-loader.cjs");
console.log(typeof loader.load, loader.load({ k: 1 }));
console.log(loader.dep(3));
