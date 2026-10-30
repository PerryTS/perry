// fs.createReadStream returns a real Readable: paused/flowing reads, listener
// management, lifecycle flags, and sequential fd reads.
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { Readable } from "node:stream";

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "perry-read-stream-"));
const file = path.join(dir, "data.txt");
fs.writeFileSync(file, "hello world, this is a read stream");

const settle = (stream: fs.ReadStream) =>
  new Promise<void>((resolve) => stream.once("close", () => resolve()));

async function main() {
  const flowing = fs.createReadStream(file, { highWaterMark: 8 });
  console.log("is Readable", flowing instanceof Readable, flowing.constructor.name);
  console.log(
    "members",
    ["off", "removeListener", "removeAllListeners", "read", "setEncoding", "listenerCount"]
      .map((name) => typeof (flowing as any)[name])
      .join(","),
  );
  const chunks: string[] = [];
  let ends = 0;
  flowing.on("data", (chunk) => chunks.push(String(chunk)));
  flowing.on("end", () => ends++);
  console.log("data listeners", flowing.listenerCount("data"));
  await settle(flowing);
  console.log("flowing", chunks.join("|"), "ends", ends);
  console.log("flowing flags", flowing.readableEnded, flowing.closed, flowing.destroyed);

  const paused = fs.createReadStream(file, { highWaterMark: 10 });
  const reads: string[] = [];
  paused.setEncoding("utf8");
  paused.on("readable", () => {
    let chunk;
    while ((chunk = paused.read()) !== null) reads.push(String(chunk));
  });
  await settle(paused);
  console.log("paused", reads.join("|"), paused.readableEnded);

  const sized = fs.createReadStream(file);
  const sizedReads: number[] = [];
  sized.on("readable", () => {
    let chunk;
    while ((chunk = sized.read(7)) !== null) sizedReads.push(chunk.length);
  });
  await settle(sized);
  console.log("sized reads", sizedReads.join(","));

  const listener = () => {};
  const removable = fs.createReadStream(file);
  removable.on("data", listener);
  removable.off("data", listener);
  console.log("after off", removable.listenerCount("data"));
  removable.on("error", listener);
  removable.removeAllListeners("error");
  console.log("after removeAllListeners", removable.listenerCount("error"));
  removable.destroy();
  await settle(removable);
  console.log("destroyed", removable.destroyed, removable.closed, removable.readableEnded);

  const fd = fs.openSync(file, "r");
  const sequential: string[] = [];
  const first = fs.createReadStream("", { fd, autoClose: false, highWaterMark: 12 });
  await new Promise<void>((resolve) => {
    first.on("data", (chunk) => sequential.push(String(chunk)));
    first.once("end", () => resolve());
  });
  console.log("fd without start", sequential.join("|"));
  console.log("autoClose false keeps fd", fs.fstatSync(fd).size, first.destroyed);
  fs.closeSync(fd);

  const startFd = fs.openSync(file, "r");
  const ranged = fs.createReadStream("", { fd: startFd, start: 6, end: 10, autoClose: false });
  const rangedChunks: string[] = [];
  await new Promise<void>((resolve) => {
    ranged.on("data", (chunk) => rangedChunks.push(String(chunk)));
    ranged.once("end", () => resolve());
  });
  console.log("fd with start/end", rangedChunks.join("|"));
  fs.closeSync(startFd);

  const empty = path.join(dir, "empty.txt");
  fs.writeFileSync(empty, "");
  const emptyStream = fs.createReadStream(empty);
  let emptyChunks = 0;
  emptyStream.on("data", () => emptyChunks++);
  await settle(emptyStream);
  console.log("empty", emptyChunks, emptyStream.readableEnded);

  const missing = fs.createReadStream(path.join(dir, "missing.txt"));
  const missingClosed = settle(missing);
  const failure: any = await new Promise((resolve) => missing.once("error", resolve));
  await missingClosed;
  console.log("missing", failure.code, failure.syscall, missing.destroyed, missing.closed);

  const opened: string[] = [];
  const byPath = fs.createReadStream(file);
  byPath.on("open", (openedFd) => opened.push(typeof openedFd));
  byPath.on("ready", () => opened.push("ready"));
  byPath.resume();
  await settle(byPath);
  console.log("open events", opened.join(","));

  let iterated = 0;
  for await (const chunk of fs.createReadStream(file, { highWaterMark: 16 })) {
    iterated += chunk.length;
  }
  console.log("async iterator bytes", iterated);

  fs.rmSync(dir, { recursive: true, force: true });
}

main();
