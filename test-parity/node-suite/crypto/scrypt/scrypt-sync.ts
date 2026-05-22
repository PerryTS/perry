import crypto from "node:crypto";
// scryptSync is deterministic for fixed inputs.
console.log(crypto.scryptSync("pw", "salt", 16).toString("hex"));
