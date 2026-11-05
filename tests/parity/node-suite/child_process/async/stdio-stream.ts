import { fork, spawn } from "node:child_process";
import * as fs from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function slot(value: any): string {
  return value === null ? "null" : typeof value;
}

function waitOpen(stream: any): Promise<void> {
  if (typeof stream.fd === "number") return Promise.resolve();
  return new Promise((resolve, reject) => {
    stream.once("open", () => resolve());
    stream.once("error", reject);
  });
}

function waitClose(child: any): Promise<number | null> {
  return new Promise((resolve) => child.on("close", (code: number | null) => resolve(code)));
}

function read(path: string): string {
  return fs.readFileSync(path, "utf8");
}

const base = join(tmpdir(), `perry-stdio-stream-${process.pid}`);
const inPath = `${base}-in.txt`;
const outPath = `${base}-out.txt`;
const errPath = `${base}-err.txt`;
const childFile = `${base}-child.js`;
const forkOutPath = `${base}-fork-out.txt`;

fs.writeFileSync(inPath, "stream-stdin");
fs.writeFileSync(outPath, "");
fs.writeFileSync(errPath, "");
fs.writeFileSync(forkOutPath, "");
fs.writeFileSync(
  childFile,
  [
    "process.stdout.write('fork-stream-out');",
    "process.on('message', () => { if (process.send) process.send({ ok: true }); process.exit(0); });",
  ].join(""),
);

const stdin = fs.createReadStream(inPath);
const stdout = fs.createWriteStream(outPath);
const stderr = fs.createWriteStream(errPath);
await Promise.all([waitOpen(stdin), waitOpen(stdout), waitOpen(stderr)]);

const child = spawn("sh", ["-c", "cat; printf stream-err >&2"], {
  stdio: [stdin, stdout, stderr],
});
console.log("spawn props:", slot(child.stdin), slot(child.stdout), slot(child.stderr));
console.log("spawn stdio:", child.stdio.map(slot).join(","));
console.log("spawn close:", await waitClose(child));
stdout.end();
stderr.end();
stdin.close();
await Promise.all([
  new Promise((resolve) => stdout.on("close", resolve)),
  new Promise((resolve) => stderr.on("close", resolve)),
]);
console.log("spawn stdout file:", read(outPath));
console.log("spawn stderr file:", read(errPath));

const forkStdout = fs.createWriteStream(forkOutPath);
await waitOpen(forkStdout);
const forked = fork(childFile, [], { stdio: ["ignore", forkStdout, "ignore", "ipc"] });
console.log("fork props:", slot(forked.stdin), slot(forked.stdout), slot(forked.stderr));
console.log("fork stdio:", forked.stdio.map(slot).join(","));
console.log("fork channel:", typeof forked.channel);
const message: any = await new Promise((resolve) => {
  forked.on("message", resolve);
  forked.send({ ping: true });
});
console.log("fork ipc:", message.ok);
console.log("fork close:", await waitClose(forked));
forkStdout.end();
await new Promise((resolve) => forkStdout.on("close", resolve));
console.log("fork stdout file:", read(forkOutPath));

for (const path of [inPath, outPath, errPath, childFile, forkOutPath]) {
  try {
    fs.unlinkSync(path);
  } catch {}
}
