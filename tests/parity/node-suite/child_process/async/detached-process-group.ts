import { fork, spawn, spawnSync } from "node:child_process";
import { writeFileSync, unlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

if (process.platform === "win32") {
  console.log("detached unix skipped:", true);
  process.exit(0);
}

const shellGroupScript =
  'pid=$$; pgid=$(ps -o pgid= -p "$$" | tr -d " "); printf "%s:%s\\n" "$pid" "$pgid"';

function pgidMatchesPid(line: string): boolean {
  const [pid, pgid] = line.trim().split(":");
  return pid.length > 0 && pid === pgid;
}

function closeCode(child: any): Promise<number | null> {
  return new Promise((resolve) => child.on("close", (code: number | null) => resolve(code)));
}

async function runSpawn() {
  const child = spawn("sh", ["-c", shellGroupScript], {
    detached: true,
    stdio: ["ignore", "pipe", "ignore"],
  });
  let stdout = "";
  child.stdout.on("data", (chunk: Buffer) => {
    stdout += chunk.toString("utf8");
  });
  const code = await closeCode(child);
  console.log("spawn detached same pgid:", pgidMatchesPid(stdout));
  console.log("spawn detached close:", code);
}

function runSpawnSync() {
  const result = spawnSync("sh", ["-c", shellGroupScript], {
    detached: true,
    encoding: "utf8",
  });
  console.log("spawnSync detached same pgid:", pgidMatchesPid(result.stdout));
  console.log("spawnSync detached status:", result.status);
}

async function runFork() {
  const childFile = join(tmpdir(), `perry-fork-detached-${process.pid}.js`);
  writeFileSync(
    childFile,
    `
const { execFileSync } = require("node:child_process");
const pid = String(process.pid);
const pgid = execFileSync("ps", ["-o", "pgid=", "-p", pid], { encoding: "utf8" }).trim();
if (process.send) process.send({ same: pid === pgid, connected: process.connected === true });
`,
  );

  const child = fork(childFile, [], {
    detached: true,
    execArgv: [],
    stdio: ["ignore", "ignore", "ignore", "ipc"],
  });
  const message: any = await new Promise((resolve) => child.on("message", resolve));
  const code = await closeCode(child);
  console.log("fork detached same pgid:", message.same);
  console.log("fork detached connected:", message.connected);
  console.log("fork detached close:", code);
  try {
    unlinkSync(childFile);
  } catch {}
}

await runSpawn();
runSpawnSync();
await runFork();
