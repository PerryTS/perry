import { WASI } from "node:wasi";

const W: any = WASI;

function createMemory(): any {
  try {
    return new WebAssembly.Memory({ initial: 1 });
  } catch {
    return {};
  }
}

function check(method: "start" | "initialize") {
  const memory = createMemory();
  let reads = 0;
  let calls = 0;
  const instance: any = {};
  Object.defineProperty(instance, "exports", {
    get() {
      reads++;
      if (reads === 1) return { memory };
      return method === "start"
        ? {
          memory,
          _start() {
            calls++;
          },
        }
        : {
          memory,
          _initialize() {
            calls++;
          },
        };
    },
  });

  try {
    console.log(
      method + ": ok",
      String(new W({ version: "preview1" })[method](instance)),
    );
  } catch (error: any) {
    console.log(method + ": throw", error?.name, error?.code || "no-code");
  }
  console.log(method + " reads/calls:", reads, calls);
}

check("start");
check("initialize");
