import { Readable } from "node:stream";
import { blob } from "node:stream/consumers";
// blob() returns Blob with .size === total bytes.
const r = Readable.from(["hello"]);
const b = await blob(r);
console.log("is Blob:", b instanceof Blob);
console.log("size:", b.size);
console.log("text:", await b.text());
