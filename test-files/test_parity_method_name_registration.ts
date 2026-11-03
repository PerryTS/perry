class Named {
  method() { return 1; }
  static staticMethod() { return 2; }
  get value() { return 3; }
  set value(v: number) {}
  ["méthode"]() { return 4; }
}
const n = new Named();
console.log(n.method.name, Named.staticMethod.name, (n as any)["méthode"].name);
const d = Object.getOwnPropertyDescriptor(n.method, "name")!;
console.log(d.value, d.writable, d.enumerable, d.configurable);
const g = Object.getOwnPropertyDescriptor(Named.prototype, "value")!;
console.log(g.get!.name, g.set!.name, n.method.bind(n).name);
function make(key: string) { return class { [key]() { return key; } }; }
for (const key of ["a", "b", "", "a"]) {
  const C = make(key); const x = new C();
  console.log((x as any)[key].name, (x as any)[key]());
}
const sym = Symbol("cue");
const obj = { [sym]() { return 5; }, [""]() { return 6; } };
console.log(obj[sym].name, obj[""].name);
Object.defineProperty(n.method, "name", { value: "renamed", configurable: true });
console.log(n.method.name, n.method.bind(n).name);
