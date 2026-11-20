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
    new W({ version: "preview1" }).start(instance);
    console.log(label + ": ok");
  } catch (error: any) {
    console.log(label + ": throw", error?.name, error?.code || "no-code");
  }
}

show("start missing", { exports: { memory } });
show("initialize present", {
  exports: { memory, _start() {}, _initialize() {} },
});
show("start nonfunction", { exports: { memory, _start: 1 } });
show("memory missing", { exports: { _start() {} } });
show("memory plain object", { exports: { memory: {}, _start() {} } });
