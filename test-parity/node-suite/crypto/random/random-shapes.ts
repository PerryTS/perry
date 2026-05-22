import * as crypto from "node:crypto";

const b = crypto.randomBytes(8);
console.log("randomBytes buffer:", Buffer.isBuffer(b));
console.log("randomBytes len:", b.length);
const uuid = crypto.randomUUID();
console.log("uuid type:", typeof uuid);
console.log("uuid len:", uuid.length);
console.log("uuid dashes:", uuid[8] + uuid[13] + uuid[18] + uuid[23]);
