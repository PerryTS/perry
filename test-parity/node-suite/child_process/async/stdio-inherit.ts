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

const inherited = spawn("sh", ["-c", "exit 0"], { stdio: "inherit" });
report("spawn inherit", inherited);
console.log("spawn inherit close:", await close(inherited));

const mixed = spawn("sh", ["-c", "cat >/dev/null"], {
  stdio: ["pipe", "inherit", "inherit"],
});
report("spawn mixed inherit", mixed);
mixed.stdin.end("mixed-input");
console.log("spawn mixed inherit close:", await close(mixed));

const childFile = join(tmpdir(), `perry-fork-stdio-inherit-${process.pid}.js`);
writeFileSync(
  childFile,
  "process.on('message', () => { if (process.send) process.send({ ok: true }); process.exit(0); }); setTimeout(() => process.exit(0), 100);",
);

const forked = fork(childFile, [], { stdio: "inherit" });
report("fork inherit", forked);
console.log("fork channel:", typeof forked.channel);
const message: any = await new Promise((resolve) => {
  forked.on("message", resolve);
  forked.send({ ping: true });
});
console.log("fork ipc:", message.ok);
console.log("fork inherit close:", await close(forked));
try {
  unlinkSync(childFile);
} catch {}
