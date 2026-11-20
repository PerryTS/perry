import * as crypto from "node:crypto";

const once = crypto.createHash("sha1").update("abcdef").digest("hex");
const split = crypto.createHash("sha1").update("abc").update("def").digest("hex");
console.log("same:", once === split);
console.log("split:", split);
