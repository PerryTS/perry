import os from "node:os";

console.log("type:", typeof os.type(), os.type().length > 0);
console.log("release:", typeof os.release(), os.release().length > 0);
console.log("platform:", typeof os.platform(), os.platform().length > 0);
console.log("arch:", typeof os.arch(), os.arch().length > 0);
