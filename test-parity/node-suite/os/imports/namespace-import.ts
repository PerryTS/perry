import * as os from "node:os";

console.log("platform fn:", typeof os.platform);
console.log("arch fn:", typeof os.arch);
console.log("EOL string:", typeof os.EOL);
