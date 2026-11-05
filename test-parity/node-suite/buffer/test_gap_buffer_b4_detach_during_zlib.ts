// B4 T3: detach the input from a data listener while deflate is operating.
import * as zlib from "node:zlib";

const owner = new ArrayBuffer(32768);
const input = Buffer.from(owner);
input.fill(0x25);
const chunks: Buffer[] = [];
let transferred = false;
const stream = zlib.createDeflate();
stream.on("data", (chunk: Buffer) => {
  chunks.push(chunk);
  if (!transferred) {
    const receiver = owner.transfer();
    transferred = true;
    console.log("transfer", owner.byteLength, receiver.byteLength);
  }
});
stream.on("end", () => {
  const restored = zlib.inflateSync(Buffer.concat(chunks));
  console.log("restored", restored.length, restored[0], restored[32767]);
  console.log("sender", input.length);
});
stream.end(input);
