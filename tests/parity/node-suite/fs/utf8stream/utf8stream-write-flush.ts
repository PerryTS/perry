import { Utf8Stream } from "node:fs";
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_utf8stream_write_flush";
try {
  fs.rmSync(ROOT, { recursive: true, force: true });
} catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

function waitClose(stream: any) {
  return new Promise<void>((resolve) => {
    stream.on("close", () => resolve());
  });
}

async function endAndWait(stream: any) {
  const closed = waitClose(stream);
  stream.end();
  await closed;
}

async function destroyAndWait(stream: any) {
  const closed = waitClose(stream);
  stream.destroy();
  await closed;
}

const appendPath = ROOT + "/append.txt";
fs.writeFileSync(appendPath, "A");
const appendStream = new Utf8Stream({ dest: appendPath, sync: true });
appendStream.write("B");
await endAndWait(appendStream);
console.log("utf8stream append default:", fs.readFileSync(appendPath, "utf8"));

const truncatePath = ROOT + "/truncate.txt";
fs.writeFileSync(truncatePath, "A");
const truncateStream = new Utf8Stream({ dest: truncatePath, append: false, sync: true });
truncateStream.write("B");
await endAndWait(truncateStream);
console.log("utf8stream append false:", fs.readFileSync(truncatePath, "utf8"));

const mkdirPath = ROOT + "/nested/deep/mkdir.txt";
const mkdirStream = new Utf8Stream({ dest: mkdirPath, mkdir: true, sync: true });
mkdirStream.write("made");
await endAndWait(mkdirStream);
console.log("utf8stream mkdir:", fs.readFileSync(mkdirPath, "utf8"));

const fdPath = ROOT + "/fd.txt";
const fd = fs.openSync(fdPath, "w");
const fdStream = new Utf8Stream({ fd, sync: true });
fdStream.write("fd");
await endAndWait(fdStream);
console.log("utf8stream fd write:", fs.readFileSync(fdPath, "utf8"));

const bytesPath = ROOT + "/bytes.txt";
const bytesStream = new Utf8Stream({ dest: bytesPath, sync: true });
bytesStream.write("a€");
await endAndWait(bytesStream);
console.log("utf8stream utf8 bytes:", Buffer.from(fs.readFileSync(bytesPath, "utf8")).toString("hex"));

const flushPath = ROOT + "/flush.txt";
const flushStream = new Utf8Stream({ dest: flushPath, sync: true, minLength: 10, maxWrite: 11 });
flushStream.write("abc");
console.log("utf8stream flush before:", fs.readFileSync(flushPath, "utf8"));
await new Promise<void>((resolve) => {
  flushStream.flush((err?: Error) => {
    console.log("utf8stream flush callback:", err == null);
    resolve();
  });
});
console.log("utf8stream flush after:", fs.readFileSync(flushPath, "utf8"));
await destroyAndWait(flushStream);

const flushSyncPath = ROOT + "/flush-sync.txt";
const flushSyncStream = new Utf8Stream({ dest: flushSyncPath, sync: true, minLength: 10, maxWrite: 11 });
flushSyncStream.write("xyz");
console.log("utf8stream flushSync before:", fs.readFileSync(flushSyncPath, "utf8"));
flushSyncStream.flushSync();
console.log("utf8stream flushSync after:", fs.readFileSync(flushSyncPath, "utf8"));
await destroyAndWait(flushSyncStream);
