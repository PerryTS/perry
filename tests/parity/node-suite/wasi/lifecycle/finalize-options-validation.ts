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

function showDirect(label: string, invoke: () => any) {
  try {
    console.log(label + ": ok", String(invoke()));
  } catch (error: any) {
    console.log(label + ": throw", error?.name, error?.code || "no-code");
  }
}

function memoryOptions(memory: any, onRead: () => void) {
  return Object.defineProperty({}, "memory", {
    get() {
      onRead();
      return memory;
    },
  });
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

const memory = createMemory();
let validReads = 0;
show("memory getter", (wasi, instance) =>
  wasi.finalizeBindings(
    instance,
    memoryOptions(memory, () => validReads++),
  ));
console.log("memory getter reads:", validReads);

let invalidReads = 0;
showDirect(
  "getter with invalid instance",
  () =>
    new W({ version: "preview1" }).finalizeBindings(
      null,
      memoryOptions(memory, () => invalidReads++),
    ),
);
console.log("invalid getter reads:", invalidReads);

let startedReads = 0;
const started = new W({ version: "preview1" });
const instance = { exports: { memory } };
showDirect("initial finalize", () => started.finalizeBindings(instance));
showDirect("getter after finalize", () =>
  started.finalizeBindings(
    instance,
    memoryOptions(memory, () => startedReads++),
  ));
console.log("started getter reads:", startedReads);
