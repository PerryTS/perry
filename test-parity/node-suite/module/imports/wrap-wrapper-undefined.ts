import * as Module from "node:module";

console.log("wrap typeof:", typeof Module.wrap);
console.log("wrap value:", String(Module.wrap));
console.log("wrapper typeof:", typeof Module.wrapper);
console.log("wrapper value:", String(Module.wrapper));
