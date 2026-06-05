const nativeErrors: Array<[string, any]> = [
  ["EvalError", EvalError],
  ["RangeError", RangeError],
  ["ReferenceError", ReferenceError],
  ["SyntaxError", SyntaxError],
  ["TypeError", TypeError],
  ["URIError", URIError],
];

for (const [name, Ctor] of nativeErrors) {
  const instance = new Ctor("boom");
  console.log(name, "ctor proto Error:", Object.getPrototypeOf(Ctor) === Error);
  console.log(name, "proto proto Error.prototype:", Object.getPrototypeOf(Ctor.prototype) === Error.prototype);
  console.log(name, "typeof Error.isPrototypeOf:", typeof Error.isPrototypeOf);
  console.log(name, "Error.isPrototypeOf:", Error.isPrototypeOf(Ctor));
  console.log(name, "Function.prototype.isPrototypeOf:", Function.prototype.isPrototypeOf(Ctor));
  console.log(name, "Object.prototype.isPrototypeOf:", Object.prototype.isPrototypeOf(Ctor));
  console.log(name, "instanceof:", instance instanceof Ctor, instance instanceof Error);
}
