import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const cipher = crypto.createCipheriv("aes-256-cbc", Buffer.alloc(32), Buffer.alloc(16));
console.log("cipher update name:", cipher.update.name);
console.log("cipher final name:", cipher.final.name);
console.log("cipher setAutoPadding name:", cipher.setAutoPadding.name);
console.log("cipher getAuthTag name:", cipher.getAuthTag.name);
console.log("cipher setAAD name:", cipher.setAAD.name);
console.log("cipher update typeof:", typeof cipher.update);

const decipher = crypto.createDecipheriv("aes-256-cbc", Buffer.alloc(32), Buffer.alloc(16));
console.log("decipher update name:", decipher.update.name);
console.log("decipher final name:", decipher.final.name);
console.log("decipher setAutoPadding name:", decipher.setAutoPadding.name);
console.log("decipher setAuthTag name:", decipher.setAuthTag.name);
console.log("decipher setAAD name:", decipher.setAAD.name);
console.log("decipher final typeof:", typeof decipher.final);
