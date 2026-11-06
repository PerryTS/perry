import { Blob } from "node:buffer";

const b = new Blob(["hi"], { type: "text/plain" });
console.log("size:", b.size);
console.log("type:", b.type);

const text = await b.text();
console.log("text:", text);

const ab = await b.arrayBuffer();
console.log("arrayBuffer.byteLength:", ab.byteLength);
