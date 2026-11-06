// ValidateAndApplyPropertyDescriptor (ECMA-262 10.1.6.3) for a non-configurable
// own property: redefining it in a forbidden way must throw a TypeError
// (`Cannot redefine property: <k>`), while spec-permitted redefinitions (a
// same-value no-op, or a value/writable change on a still-writable property)
// must succeed. A property is non-configurable either object-wide (frozen /
// sealed) or individually (`defineProperty(o, k, { configurable: false })`).
function show(label: string, fn: () => unknown) {
  try {
    console.log(label + ":", JSON.stringify(fn()) ?? "undefined");
  } catch (err: any) {
    console.log(label + ": throw", err.name + " " + err.message);
  }
}

// --- Individually non-configurable (object stays extensible) ---

show("nonconf make configurable", () => {
  const o: any = {};
  Object.defineProperty(o, "x", { value: 1, writable: true, configurable: false });
  Object.defineProperty(o, "x", { configurable: true });
  return "ok";
});

show("nonconf change enumerable", () => {
  const o: any = {};
  Object.defineProperty(o, "x", { value: 1, enumerable: true, configurable: false });
  Object.defineProperty(o, "x", { enumerable: false });
  return "ok";
});

show("nonconf data to accessor", () => {
  const o: any = {};
  Object.defineProperty(o, "x", { value: 1, configurable: false });
  Object.defineProperty(o, "x", { get() { return 2; } });
  return "ok";
});

show("nonconf accessor to data", () => {
  const o: any = {};
  Object.defineProperty(o, "x", { get() { return 1; }, configurable: false });
  Object.defineProperty(o, "x", { value: 2 });
  return "ok";
});

show("nonconf nonwritable new value", () => {
  const o: any = {};
  Object.defineProperty(o, "x", { value: 1, writable: false, configurable: false });
  Object.defineProperty(o, "x", { value: 2 });
  return "ok";
});

show("nonconf nonwritable to writable", () => {
  const o: any = {};
  Object.defineProperty(o, "x", { value: 1, writable: false, configurable: false });
  Object.defineProperty(o, "x", { writable: true });
  return "ok";
});

// --- Spec-permitted redefinitions: must NOT throw ---

show("nonconf nonwritable same value", () => {
  const o: any = {};
  Object.defineProperty(o, "x", { value: 1, writable: false, configurable: false });
  Object.defineProperty(o, "x", { value: 1 });
  return o.x;
});

show("nonconf writable new value", () => {
  const o: any = {};
  Object.defineProperty(o, "x", { value: 1, writable: true, configurable: false });
  Object.defineProperty(o, "x", { value: 9 });
  return o.x;
});

show("nonconf writable to nonwritable", () => {
  const o: any = {};
  Object.defineProperty(o, "x", { value: 1, writable: true, configurable: false });
  Object.defineProperty(o, "x", { writable: false });
  const d = Object.getOwnPropertyDescriptor(o, "x")!;
  return [d.value, d.writable, d.configurable];
});

show("conf accessor redefine ok", () => {
  const o: any = {};
  Object.defineProperty(o, "x", { value: 1, configurable: true });
  Object.defineProperty(o, "x", { get() { return 5; }, configurable: true });
  return o.x;
});

// --- Object-wide immutability (freeze / seal) ---

show("frozen redefine same value", () => {
  const o: any = { x: 1 };
  Object.freeze(o);
  Object.defineProperty(o, "x", { value: 1 });
  return o.x;
});

show("frozen redefine diff value", () => {
  const o: any = { x: 1 };
  Object.freeze(o);
  Object.defineProperty(o, "x", { value: 2 });
  return "ok";
});

show("sealed redefine value", () => {
  const o: any = { x: 1 };
  Object.seal(o);
  Object.defineProperty(o, "x", { value: 2 });
  return o.x;
});

show("sealed make configurable", () => {
  const o: any = { x: 1 };
  Object.seal(o);
  Object.defineProperty(o, "x", { configurable: true });
  return "ok";
});

show("preventExtensions add new", () => {
  const o: any = {};
  Object.preventExtensions(o);
  Object.defineProperty(o, "y", { value: 1 });
  return "ok";
});

// --- Bad-prototype message parity (setPrototypeOf / create) ---

show("setPrototypeOf bad proto", () => {
  Object.setPrototypeOf({}, 5 as any);
  return "ok";
});

show("create bad proto", () => {
  Object.create(5 as any);
  return "ok";
});
