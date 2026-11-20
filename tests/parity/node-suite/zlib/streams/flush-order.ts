import { constants, createGzip } from "node:zlib";

const events: string[] = [];
const stream = createGzip();
stream.on("data", () => {});
stream.write("a", () => events.push("write-a"));
stream.flush(constants.Z_PARTIAL_FLUSH, () => events.push("flush-a"));
stream.write("b", () => events.push("write-b"));

await new Promise<void>((resolve, reject) => {
  stream.flush(constants.Z_PARTIAL_FLUSH, () => events.push("flush-b"));
  stream.on("end", resolve);
  stream.on("error", reject);
  stream.end(() => events.push("end-callback"));
});

console.log("events:", events.join(","));
stream.destroy();
