import { PassThrough, finished } from "node:stream";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

await storage.run(
  "finished",
  () =>
    new Promise<void>((resolve, reject) => {
      const stream = new PassThrough();
      stream.resume();
      finished(stream, (error) => {
        console.log("finished callback store:", storage.getStore());
        if (error) return reject(error);
        resolve();
      });
      stream.end("payload");
    }),
);

console.log("finished outside:", String(storage.getStore()));
