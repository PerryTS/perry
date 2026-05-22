import crypto from "node:crypto";
// Hash#copy() clones the in-progress hash state so the two finish independently.
const h = crypto.createHash("sha256");
h.update("hello");
const h2 = h.copy();
h.update("!");
console.log("base:", h.digest("hex"));
console.log("copy:", h2.digest("hex"));
