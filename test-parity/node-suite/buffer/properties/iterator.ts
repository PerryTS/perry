import { Buffer } from "node:buffer";

let values = "";
for (const byte of Buffer.from([65, 66, 67])) {
  values += byte + ",";
}
console.log("for-of values:", values);
