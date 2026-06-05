function show(label: string, value: unknown) {
  console.log(label + ":", String(value));
}

function showCall(label: string, fn: () => unknown) {
  try {
    show(label, fn());
  } catch (e: any) {
    show(label, e?.name + ":" + e?.message);
  }
}

const buffer = new ArrayBuffer(4);
showCall("object proto arraybuffer", () => Object.prototype.isPrototypeOf(buffer));
showCall("arraybuffer proto via call", () => Object.prototype.isPrototypeOf.call(ArrayBuffer.prototype, buffer));
showCall("arraybuffer proto target", () => ArrayBuffer.prototype.isPrototypeOf(buffer));
showCall("arraybuffer get proto is ctor proto", () => Object.getPrototypeOf(buffer) === ArrayBuffer.prototype);
showCall("arraybuffer proto parent object", () => Object.getPrototypeOf(ArrayBuffer.prototype) === Object.prototype);

const typed = new Uint8Array(2);
showCall("typedarray get proto is ctor proto", () => Object.getPrototypeOf(typed) === Uint8Array.prototype);
showCall("typedarray proto isPrototypeOf", () => Uint8Array.prototype.isPrototypeOf(typed));
showCall("typedarray proto via call", () => Object.prototype.isPrototypeOf.call(Uint8Array.prototype, typed));
showCall(
  "typedarray proto shared parent",
  () => Object.getPrototypeOf(Uint8Array.prototype) === Object.getPrototypeOf(Int8Array.prototype),
);
showCall("typedarray proto method type", () => typeof Uint8Array.prototype.isPrototypeOf);
