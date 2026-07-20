import { exec, execFile, fork, spawn } from "node:child_process";
import { writeFileSync, unlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function valueText(value) {
  return value === null
    ? "null"
    : value === undefined
      ? "undefined"
      : String(value);
}

function errSummary(err) {
  if (!err) return "null";
  return [
    valueText(err.name),
    valueText(err.code),
    Object.keys(err).join(","),
    valueText(err.cmd),
  ].join("|");
}

function close(child) {
  return new Promise((resolve) =>
    child.on("close", (code, signal) => resolve([code, signal])),
  );
}

function invalidSignal(label, start) {
  try {
    const child = start();
    console.log(`${label} invalid: ok`);
    child?.kill?.();
  } catch (err) {
    console.log(`${label} invalid:`, err.name, err.code);
  }
}

function execWithAbort(label, run) {
  return new Promise((resolve) => {
    const controller = new AbortController();
    controller.abort();
    run(controller.signal, (err, stdout, stderr) => {
      console.log(`${label} err:`, errSummary(err));
      console.log(`${label} stdout:`, JSON.stringify(String(stdout)));
      console.log(`${label} stderr:`, JSON.stringify(String(stderr)));
      resolve();
    });
  });
}

async function liveAbort(label, child) {
  const events = [];
  child.on("spawn", () => {
    events.push(
      `spawn:${valueText(child.killed)}:${valueText(child.signalCode)}`,
    );
  });
  child.on("error", (err) => {
    events.push(
      `error:${errSummary(err)}:${valueText(child.killed)}:${valueText(child.signalCode)}`,
    );
  });
  child.on("exit", (code, signal) => {
    events.push(
      `exit:${valueText(code)}:${valueText(signal)}:${valueText(child.killed)}:${valueText(child.signalCode)}`,
    );
  });
  const [code, signal] = await close(child);
  events.push(
    `close:${valueText(code)}:${valueText(signal)}:${valueText(child.killed)}:${valueText(child.signalCode)}`,
  );
  console.log(`${label}:`, events.join(">"));
}

invalidSignal("spawn", () => spawn("node", ["-e", ""], { signal: {} }));
invalidSignal("exec", () => exec('node -e ""', { signal: {} }, () => {}));
invalidSignal("execFile", () =>
  execFile("node", ["-e", ""], { signal: {} }, () => {}),
);

await execWithAbort("exec aborted", (signal, cb) =>
  exec("sleep 1; printf exec-after", { signal, encoding: "utf8" }, cb),
);
await execWithAbort("execFile aborted", (signal, cb) =>
  execFile(
    "sh",
    ["-c", "sleep 1; printf execfile-after"],
    { signal, encoding: "utf8" },
    cb,
  ),
);

const spawnController = new AbortController();
const spawned = spawn("node", ["-e", "setTimeout(() => {}, 1000)"], {
  signal: spawnController.signal,
});
setTimeout(() => spawnController.abort(), 25);
await liveAbort("spawn aborted", spawned);

const childFile = join(tmpdir(), `perry-fork-abort-${process.pid}.js`);
writeFileSync(childFile, "setTimeout(() => {}, 1000);");
const forkController = new AbortController();
const forked = fork(childFile, [], {
  execArgv: [],
  execPath: "node",
  signal: forkController.signal,
  stdio: ["ignore", "ignore", "ignore", "ipc"],
});
setTimeout(() => forkController.abort(), 25);
await liveAbort("fork aborted", forked);
try {
  unlinkSync(childFile);
} catch {}

const lifecycle = spawn("node", ["-e", "setInterval(() => {}, 1000)"]);
try {
  await new Promise((resolve, reject) => {
    lifecycle.once("spawn", resolve);
    lifecycle.once("error", reject);
  });
  console.log(
    "kill initial:",
    lifecycle.killed,
    valueText(lifecycle.exitCode),
    valueText(lifecycle.signalCode),
  );
  console.log("kill probe:", lifecycle.kill(0), lifecycle.killed);
  console.log("kill terminate:", lifecycle.kill("SIGTERM"), lifecycle.killed);
  const lifecycleEvents = [];
  lifecycle.on("exit", (code, signal) =>
    lifecycleEvents.push(`exit:${valueText(code)}:${valueText(signal)}`),
  );
  await new Promise((resolve) =>
    lifecycle.on("close", (code, signal) => {
      lifecycleEvents.push(`close:${valueText(code)}:${valueText(signal)}`);
      resolve();
    }),
  );
  console.log("kill events:", lifecycleEvents.join(">"));
  console.log(
    "kill final:",
    valueText(lifecycle.exitCode),
    valueText(lifecycle.signalCode),
  );
  console.log("kill after close:", lifecycle.kill(), lifecycle.killed);
} finally {
  if (lifecycle.exitCode === null && lifecycle.signalCode === null)
    lifecycle.kill("SIGKILL");
}
