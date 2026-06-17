// parity-node-argv: --no-warnings

function report(label: string, run: () => unknown) {
  try {
    const value = run();
    console.log(label + ":", value === undefined ? "undefined" : String(value));
  } catch (err) {
    const e = err as any;
    console.log(label + ":", e.constructor.name, e.code, e.message);
  }
}

const objectRef = { name: "object" };
function functionRef() {}

console.log("same object:", process.finalization === process.finalization);
console.log("keys:", Object.keys(process.finalization).join(","));
console.log(
  "lengths:",
  process.finalization.register.length,
  process.finalization.registerBeforeExit.length,
  process.finalization.unregister.length,
);

report("register object", () => {
  const value = process.finalization.register(objectRef, 1 as any);
  process.finalization.unregister(objectRef);
  return value;
});
report("register function", () => {
  const value = process.finalization.register(functionRef, () => {});
  process.finalization.unregister(functionRef);
  return value;
});
report("before object", () => {
  const value = process.finalization.registerBeforeExit(objectRef, 1 as any);
  process.finalization.unregister(objectRef);
  return value;
});
report("unregister undefined", () => process.finalization.unregister(undefined as any));
report("unregister number", () => process.finalization.unregister(1 as any));
report("unregister array", () => process.finalization.unregister([] as any));

report("register no args", () => process.finalization.register());
report("register null", () => process.finalization.register(null as any, () => {}));
report("register number", () => process.finalization.register(1 as any, () => {}));
report("register boolean", () => process.finalization.register(true as any, () => {}));
report("register string", () => process.finalization.register("x" as any, () => {}));
report("register symbol", () => process.finalization.register(Symbol("x") as any, () => {}));
report("register array", () => process.finalization.register([] as any, () => {}));
