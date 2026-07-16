import { WASI } from "node:wasi";

const W: any = WASI;
const sourceArgs = ["tool", "two", "true"];
const sourceEnv: Record<string, string> = { FIRST: "one", SECOND: "two" };
const wasi = new W({ version: "preview1", args: sourceArgs, env: sourceEnv });

sourceArgs[0] = "changed";
sourceArgs.push("late");
sourceEnv.FIRST = "changed";
delete sourceEnv.SECOND;
sourceEnv.LATE = "added";
console.log("mutated args:", sourceArgs.join("|"));
console.log(
  "mutated env:",
  Object.entries(sourceEnv).map(([key, value]) => `${key}=${value}`).join("|"),
);

function createMemory(): any {
  try {
    return new WebAssembly.Memory({ initial: 1 });
  } catch {
    return {};
  }
}

const memory: any = createMemory();
wasi.start({ exports: { memory, _start() {} } });
const hasBuffer = typeof memory.buffer === "object";
console.log("memory buffer:", hasBuffer);

if (hasBuffer) {
  const view = new DataView(memory.buffer);
  const bytes = new Uint8Array(memory.buffer);
  const decoder = new TextDecoder();

  function readValues(
    countPointer: number,
    sizePointer: number,
    pointers: number,
    strings: number,
    sizesName: "args_sizes_get" | "environ_sizes_get",
    getName: "args_get" | "environ_get",
  ) {
    const sizesErrno = wasi.wasiImport[sizesName](countPointer, sizePointer);
    const count = view.getUint32(countPointer, true);
    const size = view.getUint32(sizePointer, true);
    const getErrno = wasi.wasiImport[getName](pointers, strings);
    const values = [];
    for (let index = 0; index < count; index++) {
      const start = view.getUint32(pointers + index * 4, true);
      let end = start;
      while (end < bytes.length && bytes[end] !== 0) end++;
      values.push(
        end < bytes.length
          ? decoder.decode(bytes.subarray(start, end))
          : "<unterminated>",
      );
    }
    return `${sizesErrno}/${getErrno} ${count}/${size} ${values.join("|")}`;
  }

  console.log(
    "WASI args:",
    readValues(0, 4, 8, 128, "args_sizes_get", "args_get"),
  );
  console.log(
    "WASI env:",
    readValues(32, 36, 40, 256, "environ_sizes_get", "environ_get"),
  );
}
