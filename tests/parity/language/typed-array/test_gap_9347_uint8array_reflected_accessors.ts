import { Buffer } from "node:buffer";

// Uint8Array and Buffer use Perry's BufferHeader representation and never
// enter lookup_typed_array_kind. The intrinsic %TypedArray%.prototype getters
// must nevertheless accept them, just as they accept TypedArrayHeader kinds.
const typedArrayPrototype = Object.getPrototypeOf(Uint8Array.prototype);
const names = ["length", "byteLength", "byteOffset", "buffer"] as const;
const getters: Record<string, Function> = {};
for (const name of names) {
  getters[name] = Object.getOwnPropertyDescriptor(typedArrayPrototype, name)!.get!;
}

function inspect(label: string, value: Uint8Array | Int32Array): void {
  const length = getters.length.call(value);
  const byteLength = getters.byteLength.call(value);
  const byteOffset = getters.byteOffset.call(value);
  const firstBuffer = getters.buffer.call(value);
  const secondBuffer = getters.buffer.call(value);
  console.log(
    label,
    "length", length === value.length,
    "byteLength", byteLength === value.byteLength,
    "byteOffset", byteOffset === value.byteOffset,
    "buffer", firstBuffer === value.buffer,
    "stable", firstBuffer === secondBuffer,
  );
}

const backing = new ArrayBuffer(24);
inspect("u8-own", new Uint8Array([1, 2, 3]));
inspect("u8-view", new Uint8Array(backing, 3, 5));
inspect("i32-view", new Int32Array(backing, 4, 2));
inspect("buffer", Buffer.from([4, 5, 6]));

const brandErrors: string[] = [];
for (const name of names) {
  try {
    getters[name].call({});
    brandErrors.push("none");
  } catch (error) {
    brandErrors.push((error as Error).name);
  }
}
console.log("plain-object", brandErrors.join(","));
