import * as crypto from "node:crypto";

const hash = crypto.createHash("sha256");
hash.update("prefix-");
const branchA = hash.copy();
const branchB = hash.copy();
branchA.update("a");
branchB.update("b");
hash.update("root");

console.log("copy branch a:", branchA.digest("hex"));
console.log("copy branch b:", branchB.digest("hex"));
console.log("copy root:", hash.digest("hex"));

const md5 = crypto.createHash("md5").update("x");
const md5Copy = md5.copy();
console.log("copy md5:", md5Copy.update("y").digest("hex"));
