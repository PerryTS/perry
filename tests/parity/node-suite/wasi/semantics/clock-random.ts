import { WASI } from "node:wasi";

const W: any = WASI;

function createMemory(): any {
  try {
    return new WebAssembly.Memory({ initial: 1 });
  } catch {
    return {};
  }
}
const wasi = new W({ version: "preview1" });
const memory: any = createMemory();
const instance: any = { exports: { memory } };
let binding: string;
if (typeof wasi.initialize === "function") {
  wasi.initialize(instance);
  binding = "initialize";
} else if (typeof wasi.start === "function") {
  wasi.start(instance, memory);
  binding = "start-memory";
} else {
  binding = "unavailable";
}
console.log("binding:", binding);

const hasBuffer = typeof memory.buffer === "object";
console.log("memory buffer:", hasBuffer);
if (hasBuffer) {
  const view = new DataView(memory.buffer);
  const bytes = new Uint8Array(memory.buffer);
  bytes.fill(0xa5, 32, 40);

  function callResolution(label: string, fn: () => any) {
    if (binding !== "start-memory") return String(fn());
    try {
      return String(fn());
    } catch (error: any) {
      console.log(label + ": throw", error?.name, error?.code || "no-code");
      return "throw";
    }
  }

  const realtimeResolutionErrno = callResolution(
    "realtime resolution",
    () => wasi.wasiImport.clock_res_get(0, 0),
  );
  const monotonicResolutionErrno = callResolution(
    "monotonic resolution",
    () => wasi.wasiImport.clock_res_get(1, 16),
  );
  const monotonicTimeErrno = wasi.wasiImport.clock_time_get(1, 0n, 8);
  const realtimeTimeErrno = wasi.wasiImport.clock_time_get(0, 0n, 24);
  const randomErrno = wasi.wasiImport.random_get(32, 0);
  console.log(
    "clock errno:",
    realtimeResolutionErrno,
    monotonicResolutionErrno,
    realtimeTimeErrno,
    monotonicTimeErrno,
  );
  console.log(
    "clock positive:",
    view.getBigUint64(0, true) > 0n,
    view.getBigUint64(8, true) > 0n,
    view.getBigUint64(16, true) > 0n,
    view.getBigUint64(24, true) > 0n,
  );
  console.log(
    "zero random:",
    randomErrno,
    bytes.slice(32, 40).every((value) => value === 0xa5),
  );
}
