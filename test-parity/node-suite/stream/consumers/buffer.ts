import { Buffer } from "node:buffer";
import { Readable } from "node:stream";
import { buffer } from "node:stream/consumers";

const value = await buffer(Readable.from([Buffer.from("ab"), Buffer.from("cd")]));
console.log("buffer hex:", value.toString("hex"));
console.log("buffer length:", value.length);
