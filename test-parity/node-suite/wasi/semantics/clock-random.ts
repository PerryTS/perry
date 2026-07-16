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
wasi.initialize(instance);

const hasBuffer = typeof memory.buffer === "object";
console.log("memory buffer:", hasBuffer);
if (hasBuffer) {
  const view = new DataView(memory.buffer);
  const bytes = new Uint8Array(memory.buffer);
  bytes.fill(0xa5, 32, 40);
  const resolutionErrno = wasi.wasiImport.clock_res_get(0, 0);
  const timeErrno = wasi.wasiImport.clock_time_get(1, 0n, 8);
  const randomErrno = wasi.wasiImport.random_get(32, 0);
  console.log("clock errno:", resolutionErrno, timeErrno);
  console.log(
    "clock positive:",
    view.getBigUint64(0, true) > 0n,
    view.getBigUint64(8, true) > 0n,
  );
  console.log(
    "zero random:",
    randomErrno,
    bytes.slice(32, 40).every((value) => value === 0xa5),
  );
}
