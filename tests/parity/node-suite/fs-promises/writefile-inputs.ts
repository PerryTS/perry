import * as fs from "node:fs";
import * as fsp from "node:fs/promises";
import { Readable } from "node:stream";

const ROOT = "/tmp/perry_node_suite_fs_promises_writefile_inputs";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

await fsp.writeFile(ROOT + "/iterable.txt", ["41", Buffer.from("B"), new Uint8Array([67])], { encoding: "hex" });
console.log("promises iterable content:", await fsp.readFile(ROOT + "/iterable.txt", "utf8"));

async function* asyncChunks() {
  yield "D";
  yield Buffer.from("E");
}
await fsp.writeFile(ROOT + "/async-iterable.txt", asyncChunks());
console.log("promises async iterable content:", await fsp.readFile(ROOT + "/async-iterable.txt", "utf8"));

await fsp.writeFile(ROOT + "/readable-from.txt", Readable.from(["F", Buffer.from("G")]));
console.log("promises Readable.from content:", await fsp.readFile(ROOT + "/readable-from.txt", "utf8"));

await fsp.writeFile(ROOT + "/source.txt", "HI");
await fsp.writeFile(ROOT + "/readstream.txt", fs.createReadStream(ROOT + "/source.txt"));
console.log("promises createReadStream content:", await fsp.readFile(ROOT + "/readstream.txt", "utf8"));

try {
  await fsp.writeFile(ROOT + "/invalid-chunk.txt", ["ok", 7 as any]);
} catch (err: any) {
  console.log("promises invalid chunk:", err.name, err.code);
}

try {
  await fsp.writeFile(ROOT + "/invalid-direct.txt", 7 as any);
} catch (err: any) {
  console.log("promises invalid direct:", err.name, err.code, fs.existsSync(ROOT + "/invalid-direct.txt"));
}

const pre = new AbortController();
pre.abort();
try {
  await fsp.writeFile(ROOT + "/pre-abort.txt", ["x"], { signal: pre.signal });
} catch (err: any) {
  console.log("promises pre abort:", err.name, err.code, fs.existsSync(ROOT + "/pre-abort.txt"));
}

const mid = new AbortController();
function* abortingChunks() {
  yield "A";
  mid.abort();
  yield "B";
}
try {
  await fsp.writeFile(ROOT + "/mid-abort.txt", abortingChunks(), { signal: mid.signal });
} catch (err: any) {
  console.log("promises mid abort:", err.name, err.code);
}
console.log("promises mid abort content:", fs.existsSync(ROOT + "/mid-abort.txt") ? fs.readFileSync(ROOT + "/mid-abort.txt", "utf8") : "missing");
