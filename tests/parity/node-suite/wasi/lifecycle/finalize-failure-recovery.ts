import { WASI } from "node:wasi";

const W: any = WASI;
function createMemory(): any {
  try {
    return new WebAssembly.Memory({ initial: 1 });
  } catch {
    return {};
  }
}
function show(label: string, fn: () => any) {
  try {
    console.log(label + ": ok", String(fn()));
  } catch (error: any) {
    console.log(label + ": throw", error?.name, error?.code || "no-code");
  }
}

const memory = createMemory();
const wasi = new W({ version: "preview1" });
show("missing memory", () => wasi.finalizeBindings({ exports: {} }));
show(
  "valid after failure",
  () => wasi.finalizeBindings({ exports: { memory } }),
);
show(
  "valid after success",
  () => wasi.finalizeBindings({ exports: { memory } }),
);
