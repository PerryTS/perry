// child_process stdio pipes support paused-mode reading: a `readable` listener
// plus `read()` drains the buffered bytes (Buffer, or string after setEncoding),
// and `end` arrives only after they are consumed.
import { spawn } from "node:child_process";

type Pipe = any;

function drain(stream: Pipe, size?: number): Promise<{ parts: string[]; endedAtEnd: boolean; endedWhileReading: boolean; allBuffers: boolean }> {
  return new Promise((resolve) => {
    const parts: string[] = [];
    let endedWhileReading = false;
    let allBuffers = true;
    stream.on("readable", () => {
      let chunk;
      while ((chunk = size === undefined ? stream.read() : stream.read(size)) !== null) {
        if (!Buffer.isBuffer(chunk)) allBuffers = false;
        parts.push(String(chunk));
        if (stream.readableEnded) endedWhileReading = true;
      }
    });
    stream.once("end", () => {
      resolve({ parts, endedAtEnd: stream.readableEnded === true, endedWhileReading, allBuffers });
    });
  });
}

// stdio[3] read through readable/read/end, like Effect's NodeStream.fromReadable.
const extra = spawn("/bin/sh", ["-c", "sleep 0.2; echo ready >&3"], {
  stdio: ["ignore", "inherit", "inherit", "pipe"],
});
console.log("stdio[3]", JSON.stringify(await drain(extra.stdio[3])));

// stdout in paused mode yields Buffers.
const out = spawn("/bin/sh", ["-c", "printf 'one\\ntwo\\n'"], { stdio: ["ignore", "pipe", "inherit"] });
const outResult = await drain(out.stdout);
console.log("stdout", JSON.stringify(outResult.parts.join("")), outResult.allBuffers, outResult.endedAtEnd);

// setEncoding makes read() return strings.
const text = spawn("/bin/sh", ["-c", "sleep 0.1; printf 'caf\\303\\251'"], { stdio: ["ignore", "pipe", "inherit"] });
text.stdout!.setEncoding("utf8");
let sawString = true;
const textParts: string[] = [];
await new Promise<void>((resolve) => {
  text.stdout!.on("readable", () => {
    let chunk;
    while ((chunk = text.stdout!.read()) !== null) {
      if (typeof chunk !== "string") sawString = false;
      textParts.push(chunk);
    }
  });
  text.stdout!.once("end", () => resolve());
});
console.log("encoded", JSON.stringify(textParts.join("")), sawString);

// A sized read waits for that many bytes; at EOF it returns the remainder, and
// `end` only follows once everything was read.
const sized = spawn("/bin/sh", ["-c", "sleep 0.1; printf 'abcdef'"], { stdio: ["ignore", "pipe", "inherit"] });
const sizedResult = await drain(sized.stdout, 4);
console.log("sized", JSON.stringify(sizedResult.parts), sizedResult.endedWhileReading, sizedResult.endedAtEnd);

// read() on a stream with no buffered data returns null.
const quiet = spawn("/bin/sh", ["-c", "sleep 0.1"], { stdio: ["ignore", "pipe", "inherit"] });
console.log("empty read", quiet.stdout!.read());
await new Promise<void>((resolve) => {
  quiet.stdout!.on("readable", () => quiet.stdout!.read());
  quiet.stdout!.once("end", () => resolve());
});
console.log("empty ended", quiet.stdout!.readableEnded);

// Data events keep flowing while only a data listener is attached.
const flowing = spawn("/bin/sh", ["-c", "printf 'flow'"], { stdio: ["ignore", "pipe", "inherit"] });
const flowed: string[] = [];
flowing.stdout!.on("data", (chunk: Buffer) => flowed.push(chunk.toString("utf8")));
await new Promise<void>((resolve) => flowing.stdout!.once("end", () => resolve()));
console.log("flowing", JSON.stringify(flowed.join("")));
