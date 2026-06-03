function show(label: string, fn: () => unknown) {
  try {
    console.log(label + ":", JSON.stringify(fn()));
  } catch (err: any) {
    console.log(label + ": throw", err.name);
  }
}

show("primitive target string", () => {
  const result: any = Object.assign("ab" as any, { x: 1 });
  return [
    typeof result,
    Object.prototype.toString.call(result),
    result.valueOf(),
    result.x,
    Object.keys(result).join("|"),
    result === "ab",
  ];
});

show("primitive target number", () => {
  const result: any = Object.assign(5 as any, { x: 1 });
  return [
    typeof result,
    Object.prototype.toString.call(result),
    result.valueOf(),
    result.x,
    result === 5,
  ];
});

show("non-enumerable getter order", () => {
  const log: string[] = [];
  const source: any = {};
  Object.defineProperty(source, "hidden", { value: 1, enumerable: false });
  Object.defineProperty(source, "a", {
    enumerable: true,
    get() {
      log.push("get-a");
      return 2;
    },
  });
  source.b = 3;
  const output: any = Object.assign({}, source);
  return [Object.keys(output).join("|"), output.a, output.b, output.hidden, log.join("|")];
});

show("getter abrupt partial", () => {
  const target: any = { before: 1 };
  const source: any = {
    a: 2,
    get boom() {
      throw new Error("boom");
    },
    after: 3,
  };
  try {
    Object.assign(target, source);
  } catch (err: any) {
    return [err.name, Object.keys(target).join("|"), target.a, target.after];
  }
  return ["no throw", Object.keys(target).join("|"), target.a, target.after];
});

show("readonly target abrupt", () => {
  const target: any = {};
  Object.defineProperty(target, "a", {
    value: 1,
    writable: false,
    enumerable: true,
    configurable: true,
  });
  try {
    Object.assign(target, { a: 2, b: 3 });
  } catch (err: any) {
    return [err.name, target.a, target.b, Object.keys(target).join("|")];
  }
  return ["no throw", target.a, target.b, Object.keys(target).join("|")];
});

show("symbols enumerable only", () => {
  const visible = Symbol("visible");
  const hidden = Symbol("hidden");
  const source: any = { a: 1 };
  Object.defineProperty(source, visible, { value: 2, enumerable: true });
  Object.defineProperty(source, hidden, { value: 3, enumerable: false });
  const visibleDescriptor = Object.getOwnPropertyDescriptor(source, visible);
  const hiddenDescriptor = Object.getOwnPropertyDescriptor(source, hidden);
  const output: any = Object.assign({}, source);
  return [
    output.a,
    output[visible],
    output[hidden],
    Object.getOwnPropertySymbols(output).length,
    visibleDescriptor?.enumerable,
    hiddenDescriptor?.enumerable,
  ];
});

show("source string order", () => {
  const output: any = Object.assign({ z: 0 }, "xy" as any, { a: 1 });
  return [Object.keys(output).join("|"), output[0], output[1], output.a];
});
