import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_filehandle_writefile_inputs";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });

const iterable = await fsp.open(ROOT + "/iterable.txt", "w+");
await iterable.writeFile(["41", "42"], { encoding: "hex" });
await iterable.close();
console.log("fh iterable content:", await fsp.readFile(ROOT + "/iterable.txt", "utf8"));

async function* asyncChunks() {
  yield "C";
  yield new Uint8Array([68]);
}
const asyncHandle = await fsp.open(ROOT + "/async.txt", "w+");
await asyncHandle.writeFile(asyncChunks());
await asyncHandle.close();
console.log("fh async iterable content:", await fsp.readFile(ROOT + "/async.txt", "utf8"));

const position = await fsp.open(ROOT + "/position.txt", "w+");
await position.writeFile("A");
await position.writeFile(["B", "C"]);
await position.close();
console.log("fh current position content:", await fsp.readFile(ROOT + "/position.txt", "utf8"));

await fsp.writeFile(ROOT + "/append.txt", "start");
const append = await fsp.open(ROOT + "/append.txt", "a");
await append.writeFile(["-x"]);
await append.close();
console.log("fh append mode content:", await fsp.readFile(ROOT + "/append.txt", "utf8"));

const invalid = await fsp.open(ROOT + "/invalid.txt", "w+");
try {
  await invalid.writeFile(["ok", 7 as any]);
} catch (err: any) {
  console.log("fh invalid chunk:", err.name, err.code);
}
await invalid.close();

const pre = new AbortController();
pre.abort();
const preHandle = await fsp.open(ROOT + "/pre-abort.txt", "w+");
try {
  await preHandle.writeFile(["x"], { signal: pre.signal });
} catch (err: any) {
  console.log("fh pre abort:", err.name, err.code);
}
await preHandle.close();
console.log("fh pre abort content:", await fsp.readFile(ROOT + "/pre-abort.txt", "utf8"));

const mid = new AbortController();
function* abortingChunks() {
  yield "A";
  mid.abort();
  yield "B";
}
const midHandle = await fsp.open(ROOT + "/mid-abort.txt", "w+");
try {
  await midHandle.writeFile(abortingChunks(), { signal: mid.signal });
} catch (err: any) {
  console.log("fh mid abort:", err.name, err.code);
}
await midHandle.close();
console.log("fh mid abort content:", await fsp.readFile(ROOT + "/mid-abort.txt", "utf8"));

const closed = await fsp.open(ROOT + "/closed.txt", "w+");
await closed.close();
try {
  await closed.writeFile("x");
} catch (err: any) {
  console.log("fh closed write:", err.code);
}
