import { WASI } from "node:wasi";

const W: any = WASI;

function createMemory(): any {
  try {
    return new WebAssembly.Memory({ initial: 1 });
  } catch {
    return {};
  }
}

function show(label: string, invoke: (wasi: any, instance: any) => any) {
  const wasi = new W({ version: "preview1" });
  const instance = { exports: { memory: createMemory() } };
  try {
    console.log(label + ": ok", String(invoke(wasi, instance)));
  } catch (error: any) {
    console.log(label + ": throw", error?.name, error?.code || "no-code");
  }
}

show("options omitted", (wasi, instance) => wasi.finalizeBindings(instance));
show(
  "memory undefined",
  (wasi, instance) => wasi.finalizeBindings(instance, { memory: undefined }),
);
show(
  "memory null",
  (wasi, instance) => wasi.finalizeBindings(instance, { memory: null }),
);
show(
  "memory plain object",
  (wasi, instance) => wasi.finalizeBindings(instance, { memory: {} }),
);
show("options null", (wasi, instance) => wasi.finalizeBindings(instance, null));
