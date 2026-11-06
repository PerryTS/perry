import { Writable } from "node:stream";
// write(Buffer) — sink receives the Buffer; encoding arg is 'buffer'.
let receivedEnc: any = null;
let receivedBuf: any = null;
const w = new Writable({
  write(c, enc, cb) {
    receivedEnc = enc;
    receivedBuf = c;
    cb();
  },
});
w.write(Buffer.from("hi"));
w.end();
w.on("finish", () => {
  console.log("enc:", receivedEnc);
  console.log("isBuffer:", Buffer.isBuffer(receivedBuf));
});
