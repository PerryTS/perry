import * as os from "node:os";

console.log("dlopen object:", os.constants.dlopen !== null && typeof os.constants.dlopen === "object");
console.log("RTLD_LAZY number:", typeof os.constants.dlopen.RTLD_LAZY === "number");
console.log("RTLD_NOW number:", typeof os.constants.dlopen.RTLD_NOW === "number");
