import { Hash, createHash } from "node:crypto";

const direct = Hash("sha256").update("data").digest("hex");
const viaCreate = createHash("sha256").update("data").digest("hex");
console.log("hash constructor same:", direct === viaCreate);
console.log("hash constructor digest:", direct);
