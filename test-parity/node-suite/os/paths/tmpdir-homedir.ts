import * as os from "node:os";

console.log("tmpdir string:", typeof os.tmpdir() === "string");
console.log("tmpdir nonempty:", os.tmpdir().length > 0);
console.log("homedir string:", typeof os.homedir() === "string");
