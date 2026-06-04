import { execFileSync, execSync, spawnSync } from "node:child_process";
import * as fs from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function slot(value: any): string {
  if (value === null) return "null";
  if (Buffer.isBuffer(value)) return `buffer:${value.toString("utf8")}`;
  return `${typeof value}:${String(value)}`;
}

function waitOpen(stream: any): Promise<void> {
  if (typeof stream.fd === "number") return Promise.resolve();
  return new Promise((resolve, reject) => {
    stream.once("open", () => resolve());
    stream.once("error", reject);
  });
}

function closeStream(stream: any): Promise<void> {
  if (typeof stream.end === "function") {
    stream.end();
  } else if (typeof stream.close === "function") {
    stream.close();
  }
  return new Promise((resolve) => stream.once("close", () => resolve()));
}

function reportThrow(label: string, action: () => any) {
  try {
    console.log(`${label} value:`, slot(action()));
  } catch (err: any) {
    console.log(`${label} status:`, err.status);
    console.log(`${label} stdout:`, slot(err.stdout));
    console.log(`${label} stderr:`, slot(err.stderr));
    console.log(`${label} output:`, err.output.map(slot).join("|"));
  }
}

const base = join(tmpdir(), `perry-sync-stdio-stream-${process.pid}`);
const spawnInPath = `${base}-spawn-in.txt`;
const spawnOutPath = `${base}-spawn-out.txt`;
const spawnErrPath = `${base}-spawn-err.txt`;
const execOutPath = `${base}-exec-out.txt`;
const execErrPath = `${base}-exec-err.txt`;
const fileOutPath = `${base}-file-out.txt`;
const fileErrPath = `${base}-file-err.txt`;

fs.writeFileSync(spawnInPath, "sync-stdin");
for (const path of [spawnOutPath, spawnErrPath, execOutPath, execErrPath, fileOutPath, fileErrPath]) {
  fs.writeFileSync(path, "");
}

const spawnIn = fs.createReadStream(spawnInPath);
const spawnOut = fs.createWriteStream(spawnOutPath);
const spawnErr = fs.createWriteStream(spawnErrPath);
await Promise.all([waitOpen(spawnIn), waitOpen(spawnOut), waitOpen(spawnErr)]);
const spawnResult = spawnSync("sh", ["-c", "cat; printf sync-err >&2"], {
  encoding: "utf8",
  stdio: [spawnIn, spawnOut, spawnErr],
});
console.log("spawnSync status:", spawnResult.status);
console.log("spawnSync stdout:", slot(spawnResult.stdout));
console.log("spawnSync stderr:", slot(spawnResult.stderr));
console.log("spawnSync output:", spawnResult.output.map(slot).join("|"));
await Promise.all([closeStream(spawnIn), closeStream(spawnOut), closeStream(spawnErr)]);
console.log("spawnSync stdout file:", fs.readFileSync(spawnOutPath, "utf8"));
console.log("spawnSync stderr file:", fs.readFileSync(spawnErrPath, "utf8"));

const execOut = fs.createWriteStream(execOutPath);
const execErr = fs.createWriteStream(execErrPath);
await Promise.all([waitOpen(execOut), waitOpen(execErr)]);
console.log(
  "execSync value:",
  slot(
    execSync("printf exec-out; printf exec-err >&2", {
      encoding: "utf8",
      stdio: ["ignore", execOut, execErr],
    }),
  ),
);
await Promise.all([closeStream(execOut), closeStream(execErr)]);
console.log("execSync stdout file:", fs.readFileSync(execOutPath, "utf8"));
console.log("execSync stderr file:", fs.readFileSync(execErrPath, "utf8"));

const fileOut = fs.createWriteStream(fileOutPath);
const fileErr = fs.createWriteStream(fileErrPath);
await Promise.all([waitOpen(fileOut), waitOpen(fileErr)]);
reportThrow("execFileSync throw", () =>
  execFileSync("sh", ["-c", "printf file-out; printf file-err >&2; exit 6"], {
    encoding: "utf8",
    stdio: ["ignore", fileOut, fileErr],
  }),
);
await Promise.all([closeStream(fileOut), closeStream(fileErr)]);
console.log("execFileSync stdout file:", fs.readFileSync(fileOutPath, "utf8"));
console.log("execFileSync stderr file:", fs.readFileSync(fileErrPath, "utf8"));

for (const path of [
  spawnInPath,
  spawnOutPath,
  spawnErrPath,
  execOutPath,
  execErrPath,
  fileOutPath,
  fileErrPath,
]) {
  try {
    fs.unlinkSync(path);
  } catch {}
}
