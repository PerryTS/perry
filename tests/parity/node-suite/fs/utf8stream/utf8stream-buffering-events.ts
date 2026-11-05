import { Utf8Stream } from "node:fs";
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_utf8stream_buffering_events";
try {
  fs.rmSync(ROOT, { recursive: true, force: true });
} catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

function waitClose(stream: any) {
  return new Promise<void>((resolve) => {
    stream.on("close", () => resolve());
  });
}

const asyncPath = ROOT + "/async-default.txt";
const asyncStream = new Utf8Stream({ dest: asyncPath });
console.log(
  "utf8stream async initial:",
  asyncStream.fd,
  asyncStream.file === null,
  asyncStream.writing,
  asyncStream.sync,
);
await new Promise<void>((resolve) => asyncStream.on("ready", () => resolve()));
console.log(
  "utf8stream async ready:",
  asyncStream.fd >= 0,
  asyncStream.file === asyncPath,
  asyncStream.writing,
);
const asyncDestroyReturn = asyncStream.destroy();
console.log("utf8stream async destroy return:", asyncDestroyReturn === undefined);

const customAsyncPath = ROOT + "/custom/nested/async-custom.txt";
const customCalls: string[] = [];
const customFs = {
  mkdir(path: fs.PathLike, options: any, cb: (err?: NodeJS.ErrnoException | null) => void) {
    customCalls.push("mkdir:" + Boolean(options?.recursive));
    setImmediate(() => fs.mkdir(path, options, cb));
  },
  mkdirSync() {
    customCalls.push("mkdirSync");
    throw new Error("mkdirSync should not be called for sync:false Utf8Stream");
  },
  open(path: fs.PathLike, flags: string, mode: any, cb: (err: NodeJS.ErrnoException | null, fd?: number) => void) {
    customCalls.push("open:" + flags + ":" + (mode === undefined));
    setImmediate(() => fs.open(path, flags, mode, cb));
  },
  openSync() {
    customCalls.push("openSync");
    throw new Error("openSync should not be called for sync:false Utf8Stream");
  },
};
const customAsyncStream = new Utf8Stream({
  dest: customAsyncPath,
  mkdir: true,
  fs: customFs as any,
});
console.log(
  "utf8stream custom async initial:",
  customAsyncStream.fd,
  customAsyncStream.file === null,
  customAsyncStream.writing,
);
await new Promise<void>((resolve) => customAsyncStream.on("ready", () => resolve()));
console.log(
  "utf8stream custom async ready:",
  customAsyncStream.fd >= 0,
  customAsyncStream.file === customAsyncPath,
  customAsyncStream.writing,
);
console.log("utf8stream custom async calls:", customCalls.join(","));

const partialCustomDir = ROOT + "/partial-custom/nested";
const partialCustomPath = partialCustomDir + "/async-custom.txt";
const partialCustomCalls: string[] = [];
const partialCustomFs = {
  open(path: fs.PathLike, flags: string, mode: any, cb: (err: NodeJS.ErrnoException | null, fd?: number) => void) {
    partialCustomCalls.push("open:" + flags + ":" + (mode === undefined));
    setImmediate(() => fs.open(path, flags, mode, cb));
  },
  openSync() {
    partialCustomCalls.push("openSync");
    throw new Error("openSync should not be called for sync:false Utf8Stream");
  },
};
const partialCustomStream = new Utf8Stream({
  dest: partialCustomPath,
  mkdir: true,
  fs: partialCustomFs as any,
});
console.log(
  "utf8stream partial custom initial:",
  partialCustomStream.fd,
  partialCustomStream.file === null,
  partialCustomStream.writing,
);
await new Promise<void>((resolve) => partialCustomStream.on("ready", () => resolve()));
console.log(
  "utf8stream partial custom ready:",
  partialCustomStream.fd >= 0,
  partialCustomStream.file === partialCustomPath,
  partialCustomStream.writing,
  fs.existsSync(partialCustomDir),
);
console.log("utf8stream partial custom calls:", partialCustomCalls.join(","));

const eventPath = ROOT + "/events.txt";
const eventStream = new Utf8Stream({ dest: eventPath, sync: true, minLength: 1, maxWrite: 8 });
const events: string[] = [];
eventStream.on("write", (n: number) => events.push("write:" + n));
eventStream.on("drain", () => events.push("drain"));
eventStream.on("finish", () => events.push("finish"));
const eventClosed = waitClose(eventStream).then(() => events.push("close"));
eventStream.write("a");
eventStream.end();
await eventClosed;
console.log("utf8stream event order:", events.join(","));
console.log("utf8stream event content:", fs.readFileSync(eventPath, "utf8"));

const minPath = ROOT + "/min-length.txt";
const minStream = new Utf8Stream({ dest: minPath, sync: true, minLength: 5, maxWrite: 6 });
console.log("utf8stream min write returns:", minStream.write("ab"), minStream.write("cd"));
console.log("utf8stream min before:", fs.readFileSync(minPath, "utf8"));
console.log("utf8stream min trigger:", minStream.write("e"));
console.log("utf8stream min after:", fs.readFileSync(minPath, "utf8"));
const minClosed = waitClose(minStream);
minStream.destroy();
await minClosed;

const dropPath = ROOT + "/drop.txt";
const dropStream = new Utf8Stream({ dest: dropPath, sync: true, minLength: 5, maxLength: 4, maxWrite: 6 });
const drops: string[] = [];
dropStream.on("drop", (data: string) => drops.push("drop:" + data));
console.log("utf8stream drop returns:", dropStream.write("abc"), dropStream.write("de"));
dropStream.flushSync();
const dropClosed = waitClose(dropStream);
dropStream.destroy();
await dropClosed;
console.log("utf8stream drop events:", drops.join(","));
console.log("utf8stream drop content:", fs.readFileSync(dropPath, "utf8"));

const periodicStream = new Utf8Stream({ dest: ROOT + "/periodic.txt", sync: true, periodicFlush: 50 });
console.log("utf8stream periodic property:", periodicStream.periodicFlush);
const periodicClosed = waitClose(periodicStream);
periodicStream.destroy();
await periodicClosed;
