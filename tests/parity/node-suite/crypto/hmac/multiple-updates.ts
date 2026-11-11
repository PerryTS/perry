import * as crypto from "node:crypto";

const once = crypto.createHmac("sha1", "Node").update("some datato hmac").digest("hex");
const split = crypto.createHmac("sha1", "Node").update("some data").update("to hmac").digest("hex");
console.log("same:", once === split);
console.log("split:", split);
