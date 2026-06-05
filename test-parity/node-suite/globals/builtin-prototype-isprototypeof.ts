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
