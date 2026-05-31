import * as fs from "node:fs";
import { Buffer } from "node:buffer";

const ROOT = "/tmp/perry_node_suite_fs_open_as_blob_basic";
try {
  fs.rmSync(ROOT, { recursive: true, force: true });
} catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const file = ROOT + "/file.txt";
fs.writeFileSync(file, "hello blob");

const promise = fs.openAsBlob(file, { type: "Text/Plain" });
console.log("return promise:", promise instanceof Promise);

const blob = await promise;
console.log("blob shape:", blob.size, blob.type, typeof blob.text, typeof blob.stream);
console.log("text:", await blob.text());
console.log("arrayBuffer:", Buffer.from(await blob.arrayBuffer()).toString());
console.log("bytes:", Buffer.from(await blob.bytes()).toString());

const slice = blob.slice(1, 6, "Slice/Type");
console.log("slice:", slice.size, slice.type, await slice.text());

const reader = blob.stream().getReader();
const first = await reader.read();
const second = await reader.read();
console.log("stream first:", Buffer.from(first.value ?? []).toString(), first.done);
console.log("stream second:", second.value, second.done);
