import { AsyncLocalStorage } from "node:async_hooks";
import { deflate, inflate, deflateSync, createGzip, gunzipSync } from "node:zlib";

const storage = new AsyncLocalStorage<string>();
let successCalls = 0;
let errorCalls = 0;
await storage.run("one-shot", async () => {
  await new Promise<void>((resolve, reject) => {
    deflate("completion-payload", (error, output) => {
      successCalls++;
      if (error) return reject(error);
      console.log("completion success:", storage.getStore(), output.length > 0);
      resolve();
    });
  });
  await new Promise<void>((resolve) => {
    inflate(Buffer.from([1, 2, 3]), (error, output) => {
      errorCalls++;
      console.log("completion error:", storage.getStore(), error?.code, output === undefined);
      resolve();
    });
  });
  await new Promise<void>((resolve) => {
    inflate(deflateSync("truncated").subarray(0, 4), (error, output) => {
      errorCalls++;
      console.log("completion truncated:", storage.getStore(), error?.code, output === undefined);
      resolve();
    });
  });
});
await new Promise<void>((resolve) => setImmediate(resolve));
await new Promise<void>((resolve) => setImmediate(resolve));
console.log("completion callback counts:", successCalls, errorCalls);
console.log("completion outside:", String(storage.getStore()));

// A data listener can enqueue another write and end while the native write
// completes. All input and callbacks must remain serialized in their context.
await storage.run("reentrant-stream", () => new Promise<void>((resolve, reject) => {
  const gzip = createGzip();
  const chunks: Buffer[] = [];
  const callbacks: string[] = [];
  let reentered = false;
  gzip.on("data", (chunk) => {
    chunks.push(chunk);
    if (!reentered) {
      reentered = true;
      gzip.write("second", () => callbacks.push("second"));
      gzip.end("last");
    }
  });
  gzip.on("error", reject);
  gzip.on("end", () => {
    console.log("reentrant payload:", gunzipSync(Buffer.concat(chunks)).toString());
    console.log("reentrant callbacks:", callbacks.join(","));
    console.log("reentrant context:", storage.getStore());
    resolve();
  });
  gzip.write("first", () => callbacks.push("first"));
}));
console.log("reentrant outside:", String(storage.getStore()));
