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
  env: { FIRST: "one", SECOND: 2, BOOL: false, OMIT: undefined },
});
const memory: any = createMemory();
const instance: any = { exports: { memory } };
wasi.initialize(instance);

const hasBuffer = typeof memory.buffer === "object";
console.log("memory buffer:", hasBuffer);
if (hasBuffer) {
  const view = new DataView(memory.buffer);
  const bytes = new Uint8Array(memory.buffer);
  const sizesErrno = wasi.wasiImport.environ_sizes_get(0, 4);
  const count = view.getUint32(0, true);
  const size = view.getUint32(4, true);
  const getErrno = wasi.wasiImport.environ_get(8, 64);
  const decoder = new TextDecoder();
  const values = [];
  for (let index = 0; index < count; index++) {
    const start = view.getUint32(8 + index * 4, true);
    let end = start;
    while (end < bytes.length && bytes[end] !== 0) end++;
    values.push(
      end < bytes.length
        ? decoder.decode(bytes.subarray(start, end))
        : "<unterminated>",
    );
  }
  console.log("errno:", sizesErrno, getErrno);
  console.log("sizes:", count, size);
  console.log("values:", values.join("|"));
}
