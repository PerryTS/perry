import { fork, spawn } from "node:child_process";
import { closeSync, openSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function slot(value: any): string {
  return value === null ? "null" : typeof value;
}

function report(label: string, child: any) {
  console.log(`${label} props:`, slot(child.stdin), slot(child.stdout), slot(child.stderr));
  console.log(`${label} stdio:`, child.stdio.map(slot).join(","));
}

function close(child: any): Promise<number | null> {
  return new Promise((resolve) => child.on("close", (code: number | null) => resolve(code)));
}

function readTrim(path: string): string {
  return readFileSync(path, "utf8").trim();
}

const base = join(tmpdir(), `perry-stdio-fd-${process.pid}`);
const outPath = `${base}-out.txt`;
const errPath = `${base}-err.txt`;
const forkOutPath = `${base}-fork-out.txt`;
const forkErrPath = `${base}-fork-err.txt`;
const childFile = `${base}-child.js`;

writeFileSync(outPath, "");
writeFileSync(errPath, "");
writeFileSync(forkOutPath, "");
writeFileSync(forkErrPath, "");
writeFileSync(
  childFile,
  [
    "process.stdout.write('fork-out');",
    "process.stderr.write('fork-err');",
    "process.on('message', () => { if (process.send) process.send({ ok: true }); process.exit(0); });",
  ].join(""),
);

const outFd = openSync(outPath, "w");
const errFd = openSync(errPath, "w");
const fromFds = spawn("sh", ["-c", "printf fd-out; printf fd-err >&2"], {
  stdio: ["ignore", outFd, errFd],
});
report("spawn fd", fromFds);
console.log("spawn fd close:", await close(fromFds));
closeSync(outFd);
closeSync(errFd);
console.log("spawn fd stdout file:", readTrim(outPath));
console.log("spawn fd stderr file:", readTrim(errPath));

const forkOutFd = openSync(forkOutPath, "w");
const forkErrFd = openSync(forkErrPath, "w");
const forked = fork(childFile, [], { stdio: ["ignore", forkOutFd, forkErrFd, "ipc"] });
report("fork fd", forked);
console.log("fork channel:", typeof forked.channel);
const message: any = await new Promise((resolve) => {
  forked.on("message", resolve);
  forked.send({ ping: true });
});
console.log("fork ipc:", message.ok);
console.log("fork fd close:", await close(forked));
closeSync(forkOutFd);
closeSync(forkErrFd);
console.log("fork fd stdout file:", readTrim(forkOutPath));
console.log("fork fd stderr file:", readTrim(forkErrPath));

for (const path of [outPath, errPath, forkOutPath, forkErrPath, childFile]) {
  try {
    unlinkSync(path);
  } catch {}
}
