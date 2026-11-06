import { Writable } from "node:stream";
// Stream errors carry stable `err.code` values that user code can switch on.
// Test the three documented Writable error codes.
async function getCode(setup: (w: Writable) => void): Promise<string> {
  return await new Promise((resolve) => {
    const w = new Writable({ write(_c, _e, cb) { cb(); } });
    w.on("error", (err: any) => resolve(err && err.code));
    setup(w);
  });
}
const codeAfterEnd = await getCode((w) => { w.end("a"); w.write("b"); });
const codeAfterDestroy = await getCode((w) => { w.destroy(); w.write("after"); });
const codeNull = await getCode((w) => { w.write(null as any); });
console.log("write-after-end:", codeAfterEnd);
console.log("write-after-destroy:", codeAfterDestroy);
console.log("write-null:", codeNull);
