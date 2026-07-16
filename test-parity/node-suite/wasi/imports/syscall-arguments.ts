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

function checkInvalidCalls(prefix: string, wasi: any) {
  const imports: any = wasi.wasiImport;
  show(prefix + " args missing", () => imports.args_sizes_get());
  show(prefix + " args extra", () => imports.args_sizes_get(0, 4, 8));
  show(prefix + " args string", () => imports.args_sizes_get("0", 4));
  show(prefix + " args negative", () => imports.args_sizes_get(-1, 4));
  show(prefix + " clock number", () => imports.clock_time_get(1, 0, 8));
}

checkInvalidCalls("before", new W({ version: "preview1" }));

const bound = new W({ version: "preview1" });
const memory = createMemory();
show("bind", () => bound.start({ exports: { memory, _start() {} } }));
checkInvalidCalls("after", bound);
