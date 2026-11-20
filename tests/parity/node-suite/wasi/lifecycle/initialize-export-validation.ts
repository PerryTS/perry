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
function show(label: string, instance: any) {
  try {
    new W({ version: "preview1" }).initialize(instance);
    console.log(label + ": ok");
  } catch (error: any) {
    console.log(label + ": throw", error?.name, error?.code || "no-code");
  }
}

show("optional initialize missing", { exports: { memory } });
show("start present", { exports: { memory, _start() {}, _initialize() {} } });
show("initialize nonfunction", { exports: { memory, _initialize: 1 } });
show("memory missing", { exports: { _initialize() {} } });
show("memory plain object", { exports: { memory: {}, _initialize() {} } });
