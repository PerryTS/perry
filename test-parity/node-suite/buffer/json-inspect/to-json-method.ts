import { Buffer } from "node:buffer";

const json = Buffer.from([1, 2, 255]).toJSON();
console.log("type:", json.type);
console.log("data:", json.data.join(","));
console.log("stringify:", JSON.stringify(Buffer.from([1, 2])));
