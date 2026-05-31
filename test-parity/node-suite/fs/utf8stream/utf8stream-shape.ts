import fsDefault, { Utf8Stream } from "node:fs";
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_utf8stream_shape";
try {
  fs.rmSync(ROOT, { recursive: true, force: true });
} catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const p = ROOT + "/shape.txt";
const descriptor = Object.getOwnPropertyDescriptor(fs, "Utf8Stream");
const defaultDescriptor = Object.getOwnPropertyDescriptor(fsDefault, "Utf8Stream");

console.log("utf8stream typeof:", typeof fs.Utf8Stream, typeof Utf8Stream, typeof fsDefault.Utf8Stream);
console.log("utf8stream identity:", fs.Utf8Stream === Utf8Stream, fsDefault.Utf8Stream === fs.Utf8Stream);
console.log("utf8stream name length:", fs.Utf8Stream.name, fs.Utf8Stream.length);
console.log(
  "utf8stream namespace descriptor:",
  Object.keys(fs).includes("Utf8Stream"),
  Object.prototype.propertyIsEnumerable.call(fs, "Utf8Stream"),
  descriptor?.enumerable,
);
console.log(
  "utf8stream default descriptor:",
  Object.keys(fsDefault).includes("Utf8Stream"),
  Object.prototype.propertyIsEnumerable.call(fsDefault, "Utf8Stream"),
  defaultDescriptor?.enumerable,
  defaultDescriptor?.configurable,
  typeof defaultDescriptor?.get,
  typeof defaultDescriptor?.value,
);

try {
  (fs as any).Utf8Stream({ dest: p, sync: true });
  console.log("utf8stream direct call:", "ok");
} catch (e: any) {
  console.log("utf8stream direct call:", e instanceof TypeError, e.name, e.code);
}

const stream = new fs.Utf8Stream({ dest: p, sync: true });
console.log("utf8stream instance:", stream instanceof fs.Utf8Stream, stream instanceof Utf8Stream);
console.log(
  "utf8stream defaults:",
  stream.append,
  stream.contentMode,
  typeof stream.fd,
  stream.file === p,
  stream.fsync,
  stream.maxLength,
  stream.minLength,
  stream.mkdir,
  stream.mode === undefined,
  stream.periodicFlush,
  stream.sync,
  stream.writing,
);
console.log(
  "utf8stream methods:",
  typeof stream.write,
  typeof stream.flush,
  typeof stream.flushSync,
  typeof stream.end,
  typeof stream.destroy,
  typeof stream.reopen,
  typeof stream.on,
  typeof stream.once,
  typeof stream.addListener,
  typeof stream.off,
  typeof stream.removeListener,
  typeof stream.removeAllListeners,
  typeof stream.listenerCount,
  typeof stream.emit,
);
console.log("utf8stream dispose:", typeof stream[Symbol.dispose]);
stream[Symbol.dispose]();
