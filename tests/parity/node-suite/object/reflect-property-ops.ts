function show(label: string, value: unknown) {
  console.log(label + ":", JSON.stringify(value));
}

function throwsName(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label + ":", "no throw");
  } catch (err) {
    console.log(label + ":", (err as Error).name);
  }
}

const numericKey: any = { 1: "one" };
show("get numeric key", Reflect.get(numericKey, 1));
show("has numeric key", Reflect.has(numericKey, 1));
show("delete numeric key", Reflect.deleteProperty(numericKey, 1));
show("numeric key after delete", Object.prototype.hasOwnProperty.call(numericKey, "1"));

const objectKeyTarget: any = { dyn: "dynamic" };
const objectKey = {
  toString() {
    return "dyn";
  },
};
show("get object key", Reflect.get(objectKeyTarget, objectKey));
show("has object key", Reflect.has(objectKeyTarget, objectKey));

const numericSetTarget: any = {};
show("set numeric key", Reflect.set(numericSetTarget, 2, "two"));
show("set numeric read", numericSetTarget["2"]);

const objectSetTarget: any = {};
show("set object key", Reflect.set(objectSetTarget, objectKey, "written"));
show("set object read", objectSetTarget.dyn);

const sym = Symbol("k");
const symbolTarget: any = {};
symbolTarget[sym] = 42;
show("get symbol key", Reflect.get(symbolTarget, sym));
show("has symbol key", Reflect.has(symbolTarget, sym));
show("delete symbol key", Reflect.deleteProperty(symbolTarget, sym));
show("symbol key after delete", Reflect.get(symbolTarget, sym));

const symbolSetTarget: any = {};
show("set symbol key", Reflect.set(symbolSetTarget, sym, 9));
show("set symbol read", symbolSetTarget[sym]);

const symbolUndefinedTarget: any = {};
symbolUndefinedTarget[sym] = undefined;
show("has symbol undefined", Reflect.has(symbolUndefinedTarget, sym));

const base = {
  get value() {
    return (this as any).marker;
  },
};
show("accessor receiver", Reflect.get(base, "value", { marker: "receiver" }));

const symAccessor = Symbol("accessor");
const symbolBase = {
  get [symAccessor]() {
    return (this as any).marker;
  },
};
show(
  "symbol accessor receiver",
  Reflect.get(symbolBase, symAccessor, { marker: "symbol receiver" }),
);

throwsName("define primitive target", () =>
  Reflect.defineProperty(1 as any, "x", { value: 1 }),
);
