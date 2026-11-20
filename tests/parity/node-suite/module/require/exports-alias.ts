import { createRequire } from "node:module";

const req = createRequire(import.meta.url);
const value = req("./fixtures/exports-alias.cjs");
console.log("keys:", Object.keys(value).sort().join(","));
console.log("replacement:", value.replacement, value.before, value.after);
console.log("self reference:", value.final === value);
