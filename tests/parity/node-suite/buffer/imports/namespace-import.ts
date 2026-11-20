import * as buffer from "node:buffer";
import { Buffer } from "node:buffer";

const b = Buffer.from("ns");
console.log("namespace object:", typeof buffer);
console.log("namespace fallback from:", b.toString("utf8"));
console.log("namespace keys type:", typeof Object.keys(buffer).join(","));
