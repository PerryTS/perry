import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const h1 = crypto.createHmac("sha1", "key").update("data");
console.log("hmac first hex len:", h1.digest("hex").length);
console.log("hmac second hex empty:", h1.digest("hex") === "");

const h2 = crypto.createHmac("sha1", "key").update("data");
console.log("hmac first buffer len:", Buffer.from(h2.digest()).length);
console.log("hmac second buffer len:", Buffer.from(h2.digest()).length);

const h3 = crypto.createHmac("sha1", "key");
console.log("hmac empty first hex len:", h3.digest("hex").length);
console.log("hmac empty second hex:", h3.digest("hex"));
