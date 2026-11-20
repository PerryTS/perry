import { WASI } from "node:wasi";

const W: any = WASI;

function createMemory(): any {
  try {
    return new WebAssembly.Memory({ initial: 1 });
  } catch {
    return {};
  }
}
const memory = createMemory();
function show(label: string, fn: () => any) {
  try {
    console.log(label + ": ok", String(fn()));
  } catch (error: any) {
    console.log(label + ": throw", error?.name, error?.code || "no-code");
  }
}

const explicit = new W({ version: "preview1", args: ["tool"] });
show(
  "explicit memory",
  () => explicit.finalizeBindings({ exports: {} }, { memory }),
);
show("syscall after bind", () => explicit.wasiImport.args_sizes_get(0, 4));
show("bind second", () => explicit.finalizeBindings({ exports: { memory } }));
show(
  "missing memory",
  () => new W({ version: "preview1" }).finalizeBindings({ exports: {} }),
);
show(
  "plain memory",
  () =>
    new W({ version: "preview1" }).finalizeBindings({
      exports: { memory: {} },
    }),
);
