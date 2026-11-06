import * as os from "node:os";

console.log("release string:", typeof os.release() === "string");
console.log("release nonempty:", os.release().length > 0);
console.log("hostname string:", typeof os.hostname() === "string");
