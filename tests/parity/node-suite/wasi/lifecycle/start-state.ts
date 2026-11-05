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
const command = { exports: { memory, _start() {} } };
const reactor = { exports: { memory, _initialize() {} } };
function show(label: string, fn: () => any) {
  try {
    console.log(label + ": ok", String(fn()));
  } catch (error: any) {
    console.log(label + ": throw", error?.name, error?.code || "no-code");
  }
}

const wasi = new W({ version: "preview1" });
show("start first", () => wasi.start(command));
show("start second", () => wasi.start(command));
show("initialize after start", () => wasi.initialize(reactor));
