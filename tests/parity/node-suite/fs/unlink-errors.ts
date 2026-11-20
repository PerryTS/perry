import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_unlink_errors";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const missing = ROOT + "/missing.txt";
const dir = ROOT + "/dir";
fs.mkdirSync(dir);

function showMissing(label: string, err: unknown, expectedPath: string) {
  const fsErr = err as any;
  console.log(label + " is Error:", err instanceof Error);
  console.log(label + " code:", fsErr && fsErr.code);
  console.log(label + " syscall:", fsErr && fsErr.syscall);
  console.log(label + " path matches:", fsErr && fsErr.path === expectedPath);
}

function showDirectory(label: string, err: unknown, expectedPath: string) {
  const fsErr = err as any;
  console.log(label + " is Error:", err instanceof Error);
  console.log(label + " code ok:", fsErr && (fsErr.code === "EISDIR" || fsErr.code === "EPERM"));
  console.log(label + " syscall:", fsErr && fsErr.syscall);
  console.log(label + " path matches:", fsErr && fsErr.path === expectedPath);
}

try {
  fs.unlinkSync(missing);
  console.log("sync missing unexpectedly succeeded");
} catch (err) {
  showMissing("sync missing", err, missing);
}

try {
  fs.unlinkSync(dir);
  console.log("sync dir unexpectedly succeeded");
} catch (err) {
  showDirectory("sync dir", err, dir);
}

await new Promise<void>((resolve) => {
  fs.unlink(missing, (err) => {
    showMissing("callback missing", err, missing);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.unlink(dir, (err) => {
    showDirectory("callback dir", err, dir);
    resolve();
  });
});

try {
  await fsp.unlink(missing);
  console.log("promise missing unexpectedly resolved");
} catch (err) {
  showMissing("promise missing", err, missing);
}

try {
  await fsp.unlink(dir);
  console.log("promise dir unexpectedly resolved");
} catch (err) {
  showDirectory("promise dir", err, dir);
}

const syncFile = ROOT + "/sync.txt";
fs.writeFileSync(syncFile, "sync");
fs.unlinkSync(syncFile);
console.log("sync success removed:", !fs.existsSync(syncFile));

const callbackFile = ROOT + "/callback.txt";
fs.writeFileSync(callbackFile, "callback");
await new Promise<void>((resolve) => {
  fs.unlink(callbackFile, (err) => {
    console.log("callback success err null:", err === null);
    console.log("callback success removed:", !fs.existsSync(callbackFile));
    resolve();
  });
});

const promiseFile = ROOT + "/promise.txt";
fs.writeFileSync(promiseFile, "promise");
await fsp.unlink(promiseFile);
console.log("promise success removed:", !fs.existsSync(promiseFile));

try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
