import * as path from "node:path";

const parsed = path.parse("");
console.log("parse empty:", parsed.root, parsed.dir, parsed.base, parsed.ext, parsed.name);
console.log("format empty parse:", path.format(parsed));
console.log("format all empty:", path.format({ root: "", dir: "", base: "", ext: "", name: "" }));
