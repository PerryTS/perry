import { exec, execFile, execFileSync, fork, spawn, spawnSync } from "node:child_process";
import { writeFileSync, unlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function text(value: unknown): string {
  return value === null ? "null" : value === undefined ? "undefined" : String(value);
}

function reportSpawnSync() {
  const result = spawnSync("sh", ["-c", "sleep 1"], {
    timeout: 50,
    killSignal: "SIGKILL",
    encoding: "utf8",
  });
  console.log("spawnSync status:", text(result.status));
  console.log("spawnSync signal:", text(result.signal));
  console.log("spawnSync error:", text(result.error?.code), text(result.error?.syscall));
}

function reportExecFileSync() {
  try {
    execFileSync("sh", ["-c", "sleep 1"], {
      timeout: 50,
      killSignal: "SIGKILL",
      encoding: "utf8",
    });
    console.log("execFileSync no throw");
  } catch (error: any) {
    console.log("execFileSync caught:", error instanceof Error);
    console.log("execFileSync code:", text(error.code));
    console.log("execFileSync status:", text(error.status));
    console.log("execFileSync signal:", text(error.signal));
    console.log("execFileSync stdout:", JSON.stringify(String(error.stdout)));
    console.log("execFileSync stderr:", JSON.stringify(String(error.stderr)));
  }
}

function runBuffered(
  label: string,
  start: (callback: (error: any, stdout: unknown, stderr: unknown) => void) => void,
) {
  return new Promise<void>((resolve) => {
    start((error, stdout, stderr) => {
      console.log(`${label} error:`, error instanceof Error);
      console.log(`${label} code:`, text(error?.code));
      console.log(`${label} killed:`, text(error?.killed));
      console.log(`${label} signal:`, text(error?.signal));
      console.log(`${label} stdout:`, JSON.stringify(String(stdout)));
      console.log(`${label} stderr:`, JSON.stringify(String(stderr)));
      resolve();
    });
  });
}

function runLive(label: string, child: any) {
  return new Promise<void>((resolve) => {
    console.log(`${label} killed initial:`, child.killed);
    child.on("exit", (code: number | null, signal: string | null) => {
      console.log(`${label} exit:`, text(code), text(signal));
      console.log(`${label} killed exit:`, child.killed);
    });
    child.on("close", (code: number | null, signal: string | null) => {
      console.log(`${label} close:`, text(code), text(signal));
      console.log(`${label} killed close:`, child.killed);
      console.log(`${label} signalCode:`, text(child.signalCode));
      resolve();
    });
  });
}

reportSpawnSync();
reportExecFileSync();

await runBuffered("exec", (callback) =>
  exec("sleep 1", { timeout: 50, killSignal: "SIGKILL", encoding: "utf8" }, callback)
);
await runBuffered("execFile", (callback) =>
  execFile(
    "sh",
    ["-c", "sleep 1"],
    { timeout: 50, killSignal: "SIGKILL", encoding: "utf8" },
    callback,
  )
);

await runLive(
  "spawn",
  spawn("sh", ["-c", "sleep 1"], { timeout: 50, killSignal: "SIGKILL" }),
);

const childFile = join(tmpdir(), `perry-fork-timeout-killsignal-${process.pid}.js`);
writeFileSync(childFile, "setInterval(() => {}, 1000);");

await runLive(
  "fork",
  fork(childFile, [], {
    timeout: 50,
    killSignal: "SIGKILL",
    execArgv: [],
    stdio: ["ignore", "ignore", "ignore", "ipc"],
  }),
);

try {
  unlinkSync(childFile);
} catch {}
