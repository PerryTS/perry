import * as os from "node:os";

console.log("errno object:", os.constants.errno !== null && typeof os.constants.errno === "object");
console.log("ENOENT number:", typeof os.constants.errno.ENOENT === "number");
console.log("EACCES number:", typeof os.constants.errno.EACCES === "number");
