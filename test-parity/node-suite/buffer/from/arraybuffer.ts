import { Buffer } from "node:buffer";

const ab = new ArrayBuffer(4);
const view = new Uint8Array(ab);
view[0] = 0xde; view[1] = 0xad; view[2] = 0xbe; view[3] = 0xef;
const whole = Buffer.from(ab);
console.log("arraybuffer whole:", whole.toString("hex"));
console.log("arraybuffer length:", whole.length);
