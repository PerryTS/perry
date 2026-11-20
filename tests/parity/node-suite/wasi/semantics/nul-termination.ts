import { WASI } from "node:wasi";

const W: any = WASI;

function createMemory(): any {
  try {
    return new WebAssembly.Memory({ initial: 1 });
  } catch {
    return {};
  }
}

const wasi = new W({
  version: "preview1",
  args: ["left\0right"],
  env: { KEY: "left\0right" },
});
const memory: any = createMemory();
wasi.start({ exports: { memory, _start() {} } });

const hasBuffer = typeof memory.buffer === "object";
console.log("memory buffer:", hasBuffer);
if (hasBuffer) {
  const view = new DataView(memory.buffer);
  const bytes = new Uint8Array(memory.buffer);
  const decoder = new TextDecoder();

  function inspect(
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
    const stringLimit = Math.min(bytes.length, strings + size);
    const start = count > 0 ? view.getUint32(pointers, true) : strings;

    if (start < strings || start >= stringLimit) {
      return `${sizesErrno}/${getErrno} ${count}/${size} <invalid-pointer>`;
    }

    let end = start;
    while (end < stringLimit && bytes[end] !== 0) end++;
    if (end >= stringLimit) {
      return `${sizesErrno}/${getErrno} ${count}/${size} <unterminated>`;
    }

    const value = decoder.decode(bytes.subarray(start, end));
    const tailPayload = bytes.subarray(end + 1, stringLimit).some((byte) =>
      byte !== 0
    );
    return `${sizesErrno}/${getErrno} ${count}/${size} ${value} ${tailPayload}`;
  }

  console.log(
    "args:",
    inspect(0, 4, 8, 64, "args_sizes_get", "args_get"),
  );
  console.log(
    "env:",
    inspect(16, 20, 24, 128, "environ_sizes_get", "environ_get"),
  );
}
