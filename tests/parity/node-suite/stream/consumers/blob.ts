import { Buffer } from "node:buffer";
import { Readable } from "node:stream";
import { blob } from "node:stream/consumers";

const value = await blob(Readable.from([Buffer.from("bl"), Buffer.from("ob")]));
console.log("is Blob:", value instanceof Blob);
console.log("size:", value.size);
console.log("type:", value.type);
console.log("text:", await value.text());
console.log("arrayBuffer byteLength:", (await value.arrayBuffer()).byteLength);
console.log("bytes length:", (await value.bytes()).length);
console.log("slice is Blob:", value.slice(1, 3) instanceof Blob);
console.log("slice size:", value.slice(1, 3).size);
console.log("slice text:", await value.slice(1, 3).text());
console.log("stream function:", typeof value.stream);
