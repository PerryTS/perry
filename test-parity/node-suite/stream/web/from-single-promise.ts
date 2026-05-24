import { ReadableStream } from "node:stream/web";
// ReadableStream.from(promise) — a Promise is NOT iterable, so this should
// either throw TypeError or treat it as a thenable (depends on Node version).
let result: any = null;
try {
  const rs = (ReadableStream as any).from(Promise.resolve("value"));
  const reader = rs.getReader();
  const first = await reader.read();
  result = { value: first.value, done: first.done };
} catch (e: any) {
  result = { threw: e && e.name };
}
console.log(JSON.stringify(result));
