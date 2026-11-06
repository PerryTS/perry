import { fork, spawn } from "node:child_process";
import { writeFileSync, unlinkSync } from "node:fs";
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

const ignored = spawn("sh", ["-c", "printf ignored-out; printf ignored-err >&2"], {
  stdio: "ignore",
});
report("spawn ignore", ignored);
console.log("spawn ignore close:", await close(ignored));

const mixed = spawn("cat", [], { stdio: ["pipe", "ignore", "ignore"] });
report("spawn mixed", mixed);
mixed.stdin.end("mixed-input");
console.log("spawn mixed close:", await close(mixed));

const childFile = join(tmpdir(), `perry-fork-stdio-ignore-${process.pid}.js`);
writeFileSync(
  childFile,
  "process.on('message', () => { if (process.send) process.send({ ok: true }); process.exit(0); });",
);

const forked = fork(childFile, [], { stdio: ["ignore", "ignore", "ignore", "ipc"] });
report("fork ignore", forked);
console.log("fork channel:", typeof forked.channel);
const message: any = await new Promise((resolve) => {
  forked.on("message", resolve);
  forked.send({ ping: true });
});
console.log("fork ipc:", message.ok);
console.log("fork ignore close:", await close(forked));
try {
  unlinkSync(childFile);
} catch {}
