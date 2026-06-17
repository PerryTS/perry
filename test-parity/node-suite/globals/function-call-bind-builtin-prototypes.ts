const hasOwn = Function.prototype.call.bind(Object.prototype.hasOwnProperty);
const isEnumerable = Function.prototype.call.bind(Object.prototype.propertyIsEnumerable);

function show(label: string, value: unknown) {
  console.log(label, typeof value, String(value));
}

show("plain own", hasOwn({ a: 1 }, "a"));
show("array proto push", hasOwn(Array.prototype, "push"));
show("math abs", hasOwn(Math, "abs"));
show("object proto toString", hasOwn(Object.prototype, "toString"));
show("error proto message", hasOwn(Error.prototype, "message"));

show("object proto toString enumerable", isEnumerable(Object.prototype, "toString"));
show("error proto message enumerable", isEnumerable(Error.prototype, "message"));
