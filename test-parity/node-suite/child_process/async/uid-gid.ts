import { execFileSync, fork, spawn, spawnSync } from "node:child_process";
import { chmodSync, existsSync, writeFileSync, unlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const forkExecPath = "/usr/bin/node";
const canSetIds =
  process.platform !== "win32" &&
  typeof process.getuid === "function" &&
  typeof process.getgid === "function" &&
  process.getuid() === 0 &&
  existsSync(forkExecPath);

if (!canSetIds) {
  console.log("uid gid skipped:", true);
  process.exit(0);
}

const targetUid = 65534;
const targetGid = 65534;
const identityCode = 'console.log(`${process.getuid()}:${process.getgid()}`)';

function closeCode(child: any): Promise<number | null> {
  return new Promise((resolve) => child.on("close", (code: number | null) => resolve(code)));
}

async function runSpawn() {
  const child = spawn("node", ["-e", identityCode], {
    gid: targetGid,
    uid: targetUid,
  });
  let stdout = "";
  child.stdout.on("data", (chunk: Buffer) => {
    stdout += chunk.toString("utf8");
  });
  const code = await closeCode(child);
  console.log("spawn uid gid:", stdout.trim());
  console.log("spawn uid gid close:", code);
}

function runSpawnSync() {
  const result = spawnSync("node", ["-e", identityCode], {
    encoding: "utf8",
    gid: targetGid,
    uid: targetUid,
  });
  console.log("spawnSync uid gid:", result.stdout.trim());
  console.log("spawnSync uid gid status:", result.status);
}

function runExecFileSync() {
  const output = execFileSync("node", ["-e", identityCode], {
    encoding: "utf8",
    gid: targetGid,
    uid: targetUid,
  });
  console.log("execFileSync uid gid:", output.trim());
}

async function runFork() {
  const childFile = join(tmpdir(), `perry-fork-uid-gid-${process.pid}.js`);
  writeFileSync(
    childFile,
    "if (process.send) process.send(`${process.getuid()}:${process.getgid()}`);",
  );
  chmodSync(childFile, 0o644);

  const child = fork(childFile, [], {
    cwd: tmpdir(),
    execArgv: [],
    execPath: forkExecPath,
    gid: targetGid,
    stdio: ["ignore", "ignore", "ignore", "ipc"],
    uid: targetUid,
  });
  const message = await new Promise((resolve) => child.on("message", resolve));
  const code = await closeCode(child);
  console.log("fork uid gid:", message);
  console.log("fork uid gid close:", code);
  try {
    unlinkSync(childFile);
  } catch {}
}

await runSpawn();
runSpawnSync();
runExecFileSync();
await runFork();
