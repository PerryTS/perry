// Redefinition semantics through Object.defineProperty and
// Reflect.defineProperty: the same verdict for both entries (Object throws,
// Reflect returns false), on ordinary objects and on native handle receivers.
function attempt(label: string, f: () => unknown): void {
  try {
    console.log(label, String(f()));
  } catch (e: any) {
    console.log(label, "threw", e instanceof TypeError ? "TypeError" : String(e));
  }
}

const receivers: Array<[string, () => any]> = [
  ["plain", () => ({})],
  ["headers", () => new Headers()],
  ["function", () => function named() {}],
];

for (const [name, make] of receivers) {
  // Configurable: everything may change.
  attempt(name + " configurable", () => {
    const o = make();
    Object.defineProperty(o, "x", { value: 1, writable: false, configurable: true });
    Object.defineProperty(o, "x", { value: 2 });
    Object.defineProperty(o, "x", { get() { return 3; }, configurable: true });
    Object.defineProperty(o, "x", { value: 4, writable: true });
    return o.x + "," + JSON.stringify(Object.getOwnPropertyDescriptor(o, "x"));
  });
  // Non-configurable, writable: the value and writable:false may change.
  attempt(name + " nonconfig writable", () => {
    const o = make();
    Object.defineProperty(o, "y", { value: 1, writable: true });
    const a = Reflect.defineProperty(o, "y", { value: 2 });
    const b = Reflect.defineProperty(o, "y", { writable: false });
    const c = Reflect.defineProperty(o, "y", { writable: true });
    const d = Reflect.defineProperty(o, "y", { enumerable: true });
    return [a, b, c, d, o.y].join(",");
  });
  // Non-configurable, non-writable: only the same value.
  attempt(name + " nonconfig readonly", () => {
    const o = make();
    Object.defineProperty(o, "z", { value: NaN });
    const same = Reflect.defineProperty(o, "z", { value: NaN });
    const changed = Reflect.defineProperty(o, "z", { value: 0 });
    const accessor = Reflect.defineProperty(o, "z", { get() { return 1; } });
    return [same, changed, accessor].join(",");
  });
  attempt(name + " object readonly", () => {
    const o = make();
    Object.defineProperty(o, "z", { value: -0 });
    Object.defineProperty(o, "z", { value: 0 });
    return "no throw";
  });
  // Non-configurable accessor: the same getter only.
  attempt(name + " nonconfig accessor", () => {
    const o = make();
    const g = function () { return 7; };
    Object.defineProperty(o, "w", { get: g });
    const same = Reflect.defineProperty(o, "w", { get: g });
    const set = Reflect.defineProperty(o, "w", { set(_v: unknown) {} });
    const data = Reflect.defineProperty(o, "w", { value: 1 });
    return [same, set, data, o.w].join(",");
  });
  attempt(name + " object accessor", () => {
    const o = make();
    Object.defineProperty(o, "w", { get() { return 1; } });
    Object.defineProperty(o, "w", { value: 1 });
    return "no throw";
  });
}
