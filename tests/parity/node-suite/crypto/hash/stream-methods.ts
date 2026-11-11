import { createHash } from "node:crypto";
import { PassThrough, Readable } from "node:stream";

const expected = createHash("sha256").update("abcdef").digest("hex");

const hash = createHash("sha256");
console.log("hash write typeof:", typeof hash.write);
console.log("hash end typeof:", typeof hash.end);
console.log("hash on typeof:", typeof hash.on);
console.log("hash pipe typeof:", typeof hash.pipe);
console.log("hash setEncoding typeof:", typeof hash.setEncoding);

const chunks: string[] = [];
const events: string[] = [];
hash.setEncoding("hex");
const done = new Promise<void>((resolve, reject) => {
  hash.on("data", (chunk) => {
    events.push("data");
    chunks.push(String(chunk));
  });
  hash.on("end", () => {
    events.push("end");
    resolve();
  });
  hash.on("finish", () => events.push("finish"));
  hash.on("close", () => events.push("close"));
  hash.on("error", reject);
});
const writeResult = hash.write("abc");
hash.end("def");
await done;
await new Promise<void>((resolve) => setImmediate(resolve));
console.log("hash write boolean:", typeof writeResult === "boolean");
console.log("hash stream digest equal:", chunks.join("") === expected);
console.log("hash stream events:", events.join(","));

const piped = createHash("sha256").setEncoding("hex");
const pipedChunks: string[] = [];
const pipedDone = new Promise<void>((resolve, reject) => {
  piped.on("data", (chunk) => pipedChunks.push(String(chunk)));
  piped.on("end", resolve);
  piped.on("error", reject);
});
Readable.from(["abc", "def"]).pipe(piped);
await pipedDone;
console.log("hash readable pipe equal:", pipedChunks.join("") === expected);

const out = new PassThrough();
const outChunks: Uint8Array[] = [];
out.on("data", (chunk) => outChunks.push(chunk));
const outDone = new Promise<void>((resolve, reject) => {
  out.on("end", resolve);
  out.on("error", reject);
});
const outbound = createHash("sha256");
const pipeReturn = outbound.pipe(out);
outbound.end("abcdef");
await outDone;
console.log("hash outbound pipe returns dest:", pipeReturn === out);
console.log(
  "hash outbound pipe equal:",
  Buffer.concat(outChunks as any).toString("hex") === expected,
);
