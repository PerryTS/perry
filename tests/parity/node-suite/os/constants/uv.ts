import * as os from "node:os";

console.log("reuseaddr number:", typeof os.constants.UV_UDP_REUSEADDR === "number");
console.log("reuseaddr value:", os.constants.UV_UDP_REUSEADDR === 4);
