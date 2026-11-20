import { createHmac } from "node:crypto";
import { Readable } from "node:stream";

const expected = createHmac("sha256", "key").update("abcdef").digest("hex");

const hmac = createHmac("sha256", "key");
console.log("hmac write typeof:", typeof hmac.write);
console.log("hmac end typeof:", typeof hmac.end);
console.log("hmac on typeof:", typeof hmac.on);
console.log("hmac pipe typeof:", typeof hmac.pipe);
console.log("hmac setEncoding typeof:", typeof hmac.setEncoding);

const chunks: string[] = [];
const events: string[] = [];
hmac.setEncoding("hex");
const done = new Promise<void>((resolve, reject) => {
  hmac.on("data", (chunk) => {
    events.push("data");
    chunks.push(String(chunk));
  });
  hmac.once("end", () => {
    events.push("end");
    resolve();
  });
  hmac.on("finish", () => events.push("finish"));
  hmac.on("close", () => events.push("close"));
  hmac.on("error", reject);
});
const writeResult = hmac.write("abc");
hmac.end("def");
await done;
await new Promise<void>((resolve) => setImmediate(resolve));
console.log("hmac write boolean:", typeof writeResult === "boolean");
console.log("hmac stream digest equal:", chunks.join("") === expected);
console.log("hmac stream events:", events.join(","));

const piped = createHmac("sha256", "key").setEncoding("hex");
const pipedChunks: string[] = [];
const pipedDone = new Promise<void>((resolve, reject) => {
  piped.on("data", (chunk) => pipedChunks.push(String(chunk)));
  piped.on("end", resolve);
  piped.on("error", reject);
});
Readable.from(["abc", "def"]).pipe(piped);
await pipedDone;
console.log("hmac readable pipe equal:", pipedChunks.join("") === expected);
