import { Readable } from "node:stream";
import { text } from "node:stream/consumers";

console.log("text:", await text(Readable.from(["he", "llo"])));
