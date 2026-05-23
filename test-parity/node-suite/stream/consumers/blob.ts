import { Readable } from "node:stream";
import { blob } from "node:stream/consumers";
// stream/consumers.blob(stream) resolves to a Blob with the concatenated content.
const r = Readable.from(["hello"]);
const b = await blob(r);
console.log("is Blob:", b instanceof Blob);
console.log("size:", b.size);
