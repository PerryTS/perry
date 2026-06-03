import { execFileSync, fork, spawn, spawnSync } from "node:child_process";
import { writeFileSync, unlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function nodeArgv0Code(expected: string) {
  return `console.log(process.argv0 === ${JSON.stringify(expected)} ? "match" : process.argv0)`;
}

function trim(value: unknown) {
  return String(value).trim();
}

async function runSpawn() {
  const child = spawn("node", ["-e", nodeArgv0Code("perry-spawn-argv0")], {
    argv0: "perry-spawn-argv0",
  });
  let stdout = "";
  child.stdout.on("data", (chunk) => {
    stdout += chunk;
  });
  const status = await new Promise((resolve) => child.on("close", resolve));
  console.log("spawn argv0:", trim(stdout));
  console.log("spawn status:", status);
  console.log("spawnargs first:", child.spawnargs[0]);
  console.log("spawnargs second:", child.spawnargs[1]);
}

function runSpawnSync() {
  const result = spawnSync("node", ["-e", nodeArgv0Code("perry-spawnsync-argv0")], {
    argv0: "perry-spawnsync-argv0",
    encoding: "utf8",
  });
  console.log("spawnSync argv0:", trim(result.stdout));
  console.log("spawnSync status:", result.status);
}

function runExecFileSync() {
  const stdout = execFileSync("node", ["-e", nodeArgv0Code("perry-execfile-sync-argv0")], {
    argv0: "perry-execfile-sync-argv0",
    encoding: "utf8",
  });
  console.log("execFileSync argv0:", trim(stdout));
}

async function runFork() {
  const childFile = join(tmpdir(), `perry-fork-argv0-${process.pid}.js`);
  writeFileSync(
    childFile,
    "if (process.send) process.send({ argv0: process.argv0 });",
  );

  const child = fork(childFile, ["child-arg"], {
    argv0: "perry-fork-argv0",
    execArgv: [],
    execPath: "node",
    stdio: ["ignore", "ignore", "ignore", "ipc"],
  });
  const message = await new Promise<any>((resolve) => child.on("message", resolve));
  const status = await new Promise((resolve) => child.on("close", resolve));
  console.log("fork argv0:", message.argv0);
  console.log("fork status:", status);
  console.log("fork spawnargs first:", child.spawnargs[0]);
  console.log("fork spawnargs second suffix:", child.spawnargs[1].endsWith(".js"));
  try {
    unlinkSync(childFile);
  } catch {}
}

await runSpawn();
runSpawnSync();
runExecFileSync();
await runFork();
