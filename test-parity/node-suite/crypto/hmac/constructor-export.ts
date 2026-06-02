import { Hmac, createHmac } from "node:crypto";

const direct = Hmac("sha256", "Node").update("data").digest("hex");
const viaCreate = createHmac("sha256", "Node").update("data").digest("hex");
console.log("hmac constructor same:", direct === viaCreate);
console.log("hmac constructor digest:", direct);
