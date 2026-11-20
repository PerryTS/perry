import * as nodeModule from "node:module";
import { isBuiltin } from "node:module";

console.log("is function:", typeof nodeModule.isBuiltin === "function");
console.log("length:", nodeModule.isBuiltin.length);
console.log("fs:", nodeModule.isBuiltin("fs"));
console.log("node fs:", nodeModule.isBuiltin("node:fs"));
console.log("fs promises:", nodeModule.isBuiltin("fs/promises"));
console.log("node fs promises:", nodeModule.isBuiltin("node:fs/promises"));
console.log("internal http:", nodeModule.isBuiltin("_http_agent"));
console.log("node internal http:", nodeModule.isBuiltin("node:_http_agent"));
console.log("inspector promises:", nodeModule.isBuiltin("inspector/promises"));
console.log("node sea:", nodeModule.isBuiltin("node:sea"));
console.log("bare sea:", nodeModule.isBuiltin("sea"));
console.log("npm pkg:", nodeModule.isBuiltin("axios"));
console.log("unknown:", nodeModule.isBuiltin("not-a-real-module"));
console.log("empty:", nodeModule.isBuiltin(""));
console.log("number:", nodeModule.isBuiltin(123 as any));
console.log("null:", nodeModule.isBuiltin(null as any));

const captured = nodeModule.isBuiltin;
console.log("captured node fs:", captured("node:fs"));
console.log("named fs:", isBuiltin("fs"));
console.log("named same:", isBuiltin === nodeModule.isBuiltin);
