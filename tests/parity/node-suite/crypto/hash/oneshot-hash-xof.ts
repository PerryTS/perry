import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const shake128 = crypto.hash("shake128", "");
const shake256 = crypto.hash("shake256", "");
console.log("shake128 default:", shake128);
console.log("shake256 default:", shake256);
console.log("shake128 default ok:", shake128 === "7f9c2ba4e88f827d616045507605853e");
console.log("shake256 default ok:", shake256 === "46b9dd2b0ba88d13233b3feb743eeb243fcd52ea62b81b82b50c27646ed5762f");

console.log("shake128 b64url:", crypto.hash("shake128", "", "base64url" as any));
const buf = crypto.hash("shake256", "", "buffer" as any) as Buffer;
console.log("shake256 buffer len:", buf.length);
console.log("shake128 createHash zero len:", crypto.createHash("shake128", { outputLength: 0 } as any).update("").digest("hex").length);
console.log("shake128 createHash short:", crypto.createHash("shake128", { outputLength: 5 } as any).update("").digest("hex"));
console.log("shake256 createHash long len:", crypto.createHash("shake256", { outputLength: 64 } as any).update("message").digest("hex").length);
console.log("getHashes has shake:", crypto.getHashes().includes("shake128") && crypto.getHashes().includes("shake256"));
