import * as os from "node:os";

const info = os.userInfo();
console.log("username string:", typeof info.username === "string");
console.log("homedir string:", typeof info.homedir === "string");
console.log("shell type:", typeof info.shell === "string" || info.shell === null);
console.log("uid number:", typeof info.uid === "number");
console.log("gid number:", typeof info.gid === "number");
