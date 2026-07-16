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
    console.log(
      label + ": throw",
      error?.name,
      error?.code || "no-code",
      error?.message === "initialize failure" ? "custom" : "other",
    );
  }
}

const instance = {
  exports: {
    memory: createMemory(),
    _initialize() {
      throw new Error("initialize failure");
    },
  },
};
const wasi = new W({ version: "preview1" });
show("initialize throws", () => wasi.initialize(instance));
show("retry after throw", () => wasi.initialize(instance));
