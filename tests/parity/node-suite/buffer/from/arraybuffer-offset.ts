import { Buffer } from "node:buffer";

const ab = new ArrayBuffer(6);
const view = new Uint8Array(ab);
for (let i = 0; i < view.length; i++) {
  view[i] = i + 1;
}

const tail = Buffer.from(ab, 2);
const middle = Buffer.from(ab, 2, 3);
const empty = Buffer.from(ab, 2, 0);
console.log("offset tail:", tail.length, tail.toString("hex"));
console.log("offset length:", middle.length, middle.toString("hex"));
console.log("offset empty:", empty.length, empty.toString("hex"));
console.log("offset identity:", middle.buffer === ab);
view[3] = 0xaa;
console.log("offset source-to-buffer:", middle.toString("hex"));
middle[1] = 0xbb;
console.log("offset buffer-to-source:", Array.from(view).join(","));
middle.fill(0xee, 0, 1);
console.log("offset fill-to-source:", Array.from(view).join(","));
