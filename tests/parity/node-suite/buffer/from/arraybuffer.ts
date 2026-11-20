import { Buffer } from "node:buffer";

const ab = new ArrayBuffer(4);
const view = new Uint8Array(ab);
view[0] = 0xde; view[1] = 0xad; view[2] = 0xbe; view[3] = 0xef;
const whole = Buffer.from(ab);
console.log("arraybuffer whole:", whole.toString("hex"));
console.log("arraybuffer length:", whole.length);
console.log("arraybuffer identity:", whole.buffer === ab);
view[1] = 0x11;
console.log("arraybuffer view-to-buffer:", whole.toString("hex"));
whole[2] = 0x22;
console.log("arraybuffer buffer-to-view:", Array.from(view).join(","));
