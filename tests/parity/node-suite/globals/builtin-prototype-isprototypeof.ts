const wrapperCases: Array<[string, any, any]> = [
  ["Number", Number.prototype, new Number(5)],
  ["Boolean", Boolean.prototype, new Boolean(false)],
  ["String", String.prototype, new String("x")],
];

for (const [name, proto, instance] of wrapperCases) {
  console.log(name, "typeof", typeof proto.isPrototypeOf);
  console.log(name, "direct", proto.isPrototypeOf(instance));
  console.log(name, "borrowed", Object.prototype.isPrototypeOf.call(proto, instance));
}

console.log("Function typeof", typeof Function.prototype.isPrototypeOf);
console.log("Function direct", Function.prototype.isPrototypeOf(Number));
console.log("Function borrowed", Object.prototype.isPrototypeOf.call(Function.prototype, Number));

const typedArray = new Uint8Array(2);
console.log("Uint8Array direct", Uint8Array.prototype.isPrototypeOf(typedArray));
console.log(
  "Uint8Array borrowed",
  Object.prototype.isPrototypeOf.call(Uint8Array.prototype, typedArray),
);

const arrayBuffer = new ArrayBuffer(4);
console.log("ArrayBuffer object", Object.prototype.isPrototypeOf(arrayBuffer));
console.log("ArrayBuffer direct", ArrayBuffer.prototype.isPrototypeOf(arrayBuffer));
console.log(
  "ArrayBuffer borrowed",
  Object.prototype.isPrototypeOf.call(ArrayBuffer.prototype, arrayBuffer),
);
