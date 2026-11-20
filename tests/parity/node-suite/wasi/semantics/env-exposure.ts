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
  env: {
    FIRST: "one",
    SECOND: 2,
    BOOL: false,
    UNICODE: "café雪🙂",
    OMIT: undefined,
  },
});
const memory: any = createMemory();
const instance: any = { exports: { memory } };
let binding: string;
if (typeof wasi.initialize === "function") {
  wasi.initialize(instance);
  binding = "initialize";
} else if (typeof wasi.start === "function") {
  wasi.start(instance, memory);
  binding = "start-memory";
} else {
  binding = "unavailable";
}
console.log("binding:", binding);

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
  const pointerCapacity = (64 - 8) / 4;
  const safeCount = Math.min(count, pointerCapacity);
  const stringLimit = Math.min(bytes.length, 64 + size);
  for (let index = 0; index < safeCount; index++) {
    const start = view.getUint32(8 + index * 4, true);
    if (start < 64 || start >= stringLimit) {
      values.push("<invalid-pointer>");
      continue;
    }
    let end = start;
    while (end < stringLimit && bytes[end] !== 0) end++;
    values.push(
      end < stringLimit
        ? decoder.decode(bytes.subarray(start, end))
        : "<unterminated>",
    );
  }
  if (count > pointerCapacity) values.push("<count-overflow>");
  console.log("errno:", sizesErrno, getErrno);
  console.log("sizes:", count, size);
  console.log("values:", values.join("|"));
}
