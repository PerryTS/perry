function show(label: string, fn: () => unknown) {
  try {
    console.log(label + ":", String(fn()));
  } catch (e: any) {
    console.log(label + ":", e?.name + ":" + e?.message);
  }
}

const constructors = [
  ["Map", Map],
  ["Set", Set],
  ["WeakMap", WeakMap],
  ["WeakSet", WeakSet],
] as const;

for (const [name, Ctor] of constructors) {
  show(name + " direct call", () => (Ctor as any)());
  show(name + " rebound call", () => {
    const C = Ctor as any;
    return C();
  });
  show(name + " call method", () => (Ctor as any).call(undefined));
  show(name + " reflect apply", () => Reflect.apply(Ctor as any, {}, []));
  show(name + " rebound new tag", () => Object.prototype.toString.call(new (Ctor as any)()));
}
