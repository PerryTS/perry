import * as os from "node:os";

console.log("eol string:", typeof os.EOL === "string");
console.log("eol platform:", os.platform() === "win32" ? os.EOL === "\r\n" : os.EOL === "\n");
