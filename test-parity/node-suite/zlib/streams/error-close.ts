import { createGunzip } from "node:zlib";

const stream = createGunzip();
await new Promise<void>((resolve) => {
  stream.on("error", (error: any) => {
    console.log("error:", error.name, error.code);
    console.log(
      "closed:",
      (stream as any)._closed,
      stream.destroyed,
      stream.closed,
    );
    stream.close();
    console.log(
      "close again:",
      (stream as any)._closed,
      stream.destroyed,
      stream.closed,
    );
    resolve();
  });
  stream.end(Buffer.from("invalid"));
});
