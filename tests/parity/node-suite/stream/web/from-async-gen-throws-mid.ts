import { ReadableStream } from "node:stream/web";
// RS.from(asyncGen) where gen throws mid-iteration — read() rejects.
async function* gen() {
  yield "a";
  throw new Error("mid-fail");
}
const rs = (ReadableStream as any).from(gen());
const reader = rs.getReader();
const first = await reader.read();
let secondErr: string | null = null;
try {
  await reader.read();
} catch (e: any) {
  secondErr = e && e.message;
}
console.log("first value:", first.value);
console.log("second err:", secondErr);
