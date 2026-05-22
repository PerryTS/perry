import * as crypto from "node:crypto";

const s = "6fbf7e2948e0c2f29eaacac1733546a4af5ca482";
console.log("sha1 binary:", crypto.createHash("sha1").update(s, "binary").digest("hex"));
console.log("sha1 latin1 same:", crypto.createHash("sha1").update(s, "latin1").digest("hex"));
console.log("sha256 hex input:", crypto.createHash("sha256").update("616263", "hex").digest("hex"));
