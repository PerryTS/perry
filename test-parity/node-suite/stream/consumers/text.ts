import { Readable } from "node:stream";
import { text } from "node:stream/consumers";
// stream/consumers.text(stream) resolves to a UTF-8 decoded string.
const r = Readable.from(["hello ", "world"]);
console.log("got:", await text(r));
