import * as os from "node:os";

console.log("devNull string:", typeof os.devNull === "string");
console.log("devNull platform:", os.platform() === "win32" ? os.devNull === "\\\\.\\nul" : os.devNull === "/dev/null");
