import { WASI } from "node:wasi";

const W: any = WASI;
function createMemory(): any {
  try {
    return new WebAssembly.Memory({ initial: 1 });
  } catch {
    return {};
  }
}

const exportedMemory = createMemory();
const overrideMemory = createMemory();
const wasi = new W({ version: "preview1", args: ["tool"] });
const hasBuffers = typeof exportedMemory.buffer === "object" &&
  typeof overrideMemory.buffer === "object";

console.log("memory buffers:", hasBuffers);
try {
  console.log(
    "finalize:",
    String(
      wasi.finalizeBindings(
        { exports: { memory: exportedMemory } },
        { memory: overrideMemory },
      ),
    ),
  );
  console.log("sizes errno:", wasi.wasiImport.args_sizes_get(0, 4));
  if (hasBuffers) {
    console.log(
      "argument counts:",
      new DataView(exportedMemory.buffer).getUint32(0, true),
      new DataView(overrideMemory.buffer).getUint32(0, true),
    );
  }
} catch (error: any) {
  console.log("finalize: throw", error?.name, error?.code || "no-code");
}
