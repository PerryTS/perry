import { inspect } from "node:util";

const ab = new ArrayBuffer(4);
new Uint8Array(ab).set([1, 2, 3, 4]);
console.log("arraybuffer:", inspect(ab));
console.log("uint8:", inspect(new Uint8Array(ab, 1, 2)));
console.log("dataview:", inspect(new DataView(ab)));
