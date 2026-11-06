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

function checkMemberAccess(method: "start" | "initialize") {
  const memory = createMemory();
  const reads: string[] = [];
  let calls = 0;
  const exportsObject: any = {};
  Object.defineProperties(exportsObject, {
    memory: {
      get() {
        reads.push("memory");
        return memory;
      },
    },
    _start: {
      get() {
        reads.push("_start");
        return method === "start"
          ? () => {
            calls++;
          }
          : undefined;
      },
    },
    _initialize: {
      get() {
        reads.push("_initialize");
        return method === "initialize"
          ? () => {
            calls++;
          }
          : undefined;
      },
    },
  });

  try {
    console.log(
      method + " members: ok",
      String(
        new W({ version: "preview1" })[method]({ exports: exportsObject }),
      ),
    );
  } catch (error: any) {
    console.log(
      method + " members: throw",
      error?.name,
      error?.code || "no-code",
    );
  }
  console.log(method + " member reads/calls:", reads.join(","), calls);
}

checkMemberAccess("start");
checkMemberAccess("initialize");
