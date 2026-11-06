import processDefault, {
  availableMemory,
  constrainedMemory,
  getActiveResourcesInfo,
  hasUncaughtExceptionCaptureCallback,
  resourceUsage,
  setSourceMapsEnabled,
  setUncaughtExceptionCaptureCallback,
  threadCpuUsage,
} from "node:process";
import * as processNS from "node:process";
import { Buffer } from "node:buffer";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import * as url from "node:url";

function showThrow(label: string, fn: () => unknown): void {
  try {
    fn();
    console.log(label, "NO_THROW");
  } catch (err) {
    const e = err as { code?: string; message?: string; name?: string };
    console.log(label, e.name, e.code ?? "", (e.message ?? "").split("\n")[0]);
  }
}

function cpuShape(value: any): boolean {
  return typeof value.user === "number" && typeof value.system === "number";
}

function resourceShape(value: any): boolean {
  return typeof value.userCPUTime === "number" && typeof value.maxRSS === "number";
}

console.log(
  "helper call forms:",
  cpuShape(process.threadCpuUsage()),
  cpuShape(processDefault.threadCpuUsage()),
  cpuShape(processNS.threadCpuUsage()),
  cpuShape(threadCpuUsage()),
  typeof process.availableMemory() === "number",
  typeof processDefault.availableMemory() === "number",
  typeof processNS.availableMemory() === "number",
  typeof availableMemory() === "number",
  typeof process.constrainedMemory() === "number",
  typeof constrainedMemory() === "number",
  resourceShape(process.resourceUsage()),
  resourceShape(resourceUsage()),
  Array.isArray(process.getActiveResourcesInfo()),
  Array.isArray(getActiveResourcesInfo()),
);

const capturedActiveResources = process.getActiveResourcesInfo;
const capturedResourceUsage = process.resourceUsage;
console.log(
  "captured helpers:",
  Array.isArray(capturedActiveResources()),
  resourceShape(capturedResourceUsage()),
);

setSourceMapsEnabled(false);
console.log("source maps before:", process.sourceMapsEnabled, processDefault.sourceMapsEnabled);
setSourceMapsEnabled(true);
console.log("source maps enabled:", process.sourceMapsEnabled, processDefault.sourceMapsEnabled);
processNS.setSourceMapsEnabled(false);
console.log("source maps disabled:", process.sourceMapsEnabled, processDefault.sourceMapsEnabled);
showThrow("source maps invalid:", () => setSourceMapsEnabled(1 as any));

setUncaughtExceptionCaptureCallback(null);
console.log("capture callback none:", hasUncaughtExceptionCaptureCallback());
const capturedSetCapture = process.setUncaughtExceptionCaptureCallback;
capturedSetCapture(() => {});
console.log("capture callback set:", process.hasUncaughtExceptionCaptureCallback());
process.setUncaughtExceptionCaptureCallback(null);
showThrow("capture callback invalid:", () => setUncaughtExceptionCaptureCallback(1 as any));

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "perry-process-cohort-"));
const previousCwd = process.cwd();
const keys = [
  "PERRY_COHORT_DEFAULT",
  "PERRY_COHORT_NULL",
  "PERRY_COHORT_EXPORT",
  "PERRY_COHORT_INLINE",
  "PERRY_COHORT_HASH",
  "PERRY_COHORT_MULTI",
  "PERRY_COHORT_PATH",
  "PERRY_COHORT_BUFFER",
  "PERRY_COHORT_URL",
];
for (const key of keys) {
  delete process.env[key];
}

try {
  fs.writeFileSync(
    path.join(tmp, ".env"),
    "PERRY_COHORT_DEFAULT=omitted\n" +
      "PERRY_COHORT_NULL=nullish\n",
  );
  process.chdir(tmp);
  process.loadEnvFile();
  console.log("dotenv omitted:", process.env.PERRY_COHORT_DEFAULT);
  delete process.env.PERRY_COHORT_NULL;
  process.loadEnvFile(null as any);
  console.log("dotenv null:", process.env.PERRY_COHORT_NULL);

  const pathFile = path.join(tmp, "path.env");
  fs.writeFileSync(
    pathFile,
    "export PERRY_COHORT_EXPORT=works\n" +
      "PERRY_COHORT_INLINE=one # comment\n" +
      "PERRY_COHORT_HASH=\"two # hash\"\n" +
      "PERRY_COHORT_MULTI=\"line1\nline2\"\n",
  );
  process.loadEnvFile(pathFile);
  console.log(
    "dotenv parsed:",
    process.env.PERRY_COHORT_EXPORT,
    process.env.PERRY_COHORT_INLINE,
    process.env.PERRY_COHORT_HASH,
    JSON.stringify(process.env.PERRY_COHORT_MULTI),
  );

  const bufferFile = path.join(tmp, "buffer.env");
  fs.writeFileSync(bufferFile, "PERRY_COHORT_BUFFER=buffer-path\n");
  process.loadEnvFile(Buffer.from(bufferFile));
  console.log("dotenv buffer:", process.env.PERRY_COHORT_BUFFER);

  const urlFile = path.join(tmp, "url.env");
  fs.writeFileSync(urlFile, "PERRY_COHORT_URL=file-url\n");
  process.loadEnvFile(url.pathToFileURL(urlFile));
  console.log("dotenv url:", process.env.PERRY_COHORT_URL);
  showThrow("dotenv invalid:", () => process.loadEnvFile(1 as any));
} finally {
  process.chdir(previousCwd);
  fs.rmSync(tmp, { recursive: true, force: true });
  for (const key of keys) {
    delete process.env[key];
  }
}

function onceFn(value: string): void {
  console.log("once fired:", value);
}
process.removeAllListeners("__cohort_raw__");
process.once("__cohort_raw__", onceFn);
const raw = process.rawListeners("__cohort_raw__");
const listeners = process.listeners("__cohort_raw__");
console.log(
  "raw once wrapper:",
  typeof raw[0],
  raw[0] === onceFn,
  raw[0].listener === onceFn,
  listeners[0] === onceFn,
);
console.log("once emit:", process.emit("__cohort_raw__" as any, "value"));
console.log("once after:", process.listenerCount("__cohort_raw__"));

process.removeAllListeners("error");
showThrow("emit error missing:", () => process.emit("error" as any));
showThrow("emit error string:", () => process.emit("error" as any, "boom"));
const sameError = new Error("same");
try {
  process.emit("error" as any, sameError);
  console.log("emit error object:", false);
} catch (err) {
  console.log("emit error object:", err === sameError, (err as Error).message);
}

const beforeTimeouts = process.getActiveResourcesInfo().filter((name) => name === "Timeout").length;
const timeout = setTimeout(() => {}, 1000);
const interval = setInterval(() => {}, 1000);
const during = process.getActiveResourcesInfo();
clearTimeout(timeout);
clearInterval(interval);
const afterTimeouts = process.getActiveResourcesInfo().filter((name) => name === "Timeout").length;
console.log("active timeout resource:", during.includes("Timeout"));
console.log("active timeout cleared:", afterTimeouts <= beforeTimeouts);
