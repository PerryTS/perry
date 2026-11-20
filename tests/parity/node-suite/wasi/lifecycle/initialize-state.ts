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
const reactor = { exports: { memory, _initialize() {} } };
const command = { exports: { memory, _start() {} } };
function show(label: string, fn: () => any) {
  try {
    console.log(label + ": ok", String(fn()));
  } catch (error: any) {
    console.log(label + ": throw", error?.name, error?.code || "no-code");
  }
}

const wasi = new W({ version: "preview1" });
show("initialize first", () => wasi.initialize(reactor));
show("initialize second", () => wasi.initialize(reactor));
show("start after initialize", () => wasi.start(command));
