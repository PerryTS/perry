import { ReadableStream } from "node:stream/web";
// getReader({mode:'byob'}) on a type:'bytes' stream returns a BYOB reader.
let result: any = null;
try {
  const rs = new ReadableStream({ type: "bytes" } as any);
  const reader = (rs as any).getReader({ mode: "byob" });
  result = {
    hasRead: typeof reader.read === "function",
    hasReleaseLock: typeof reader.releaseLock === "function",
  };
} catch (e: any) {
  result = { threw: e && e.name };
}
console.log(JSON.stringify(result));
