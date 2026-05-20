import * as os from "node:os";

console.log("version string:", typeof os.version() === "string");
console.log("version nonempty:", os.version().length > 0);
