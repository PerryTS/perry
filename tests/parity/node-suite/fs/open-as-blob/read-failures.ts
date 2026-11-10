import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_open_as_blob_read_failures";
try {
  fs.rmSync(ROOT, { recursive: true, force: true });
} catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

async function reportReject(label: string, promise: Promise<unknown>) {
  try {
    await promise;
    console.log(label, "ok");
  } catch (e: any) {
    console.log(label, "reject", e.name, e.message);
  }
}

const dirBlob = await fs.openAsBlob(ROOT);
console.log("dir blob:", typeof dirBlob.size, dirBlob.type);
await reportReject("dir text", dirBlob.text());

const mutateFile = ROOT + "/mutate.txt";
fs.writeFileSync(mutateFile, "before");
const mutateBlob = await fs.openAsBlob(mutateFile);
fs.writeFileSync(mutateFile, "changed content");
console.log("mutate size remains:", mutateBlob.size);
await reportReject("mutate text", mutateBlob.text());
await reportReject("mutate bytes", mutateBlob.bytes());

const streamFile = ROOT + "/stream.txt";
fs.writeFileSync(streamFile, "stream before");
const streamBlob = await fs.openAsBlob(streamFile);
const stream = streamBlob.stream();
fs.writeFileSync(streamFile, "x");
await reportReject("stream lazy read", stream.getReader().read());
