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

for (const target of ["start", "initialize"] as const) {
  const memory = createMemory();
  const wasi = new W({ version: "preview1" });
  show(
    target + " finalize",
    () => wasi.finalizeBindings({ exports: { memory } }),
  );
  show(
    target + " after finalize",
    () =>
      target === "start"
        ? wasi.start({ exports: { memory, _start() {} } })
        : wasi.initialize({ exports: { memory, _initialize() {} } }),
  );
}

for (const source of ["start", "initialize"] as const) {
  const memory = createMemory();
  const wasi = new W({ version: "preview1" });
  show(
    source + " first",
    () =>
      source === "start"
        ? wasi.start({ exports: { memory, _start() {} } })
        : wasi.initialize({ exports: { memory, _initialize() {} } }),
  );
  show(
    "finalize after " + source,
    () => wasi.finalizeBindings({ exports: { memory } }),
  );
}
