function log(label: string, fn: () => unknown) {
  try {
    console.log(label, JSON.stringify(fn()));
  } catch (err: any) {
    console.log(label, "throw", err.name, err.message);
  }
}

log("assign mutates target", () => {
  const target: any = { a: 1 };
  const result = Object.assign(target, { b: 2 });
  return [target, result, target === result];
});

log("assign no args", () => Object.assign());

log("assign null target", () => Object.assign(null as any, { a: 1 }));

log("assign undefined target", () => Object.assign(undefined as any, { a: 1 }));

log("assign null source", () => Object.assign({ a: 1 }, null, undefined, { b: 2 }));

log("assign primitives", () => Object.assign({}, "ab", 5, true));

log("assign symbols", () => {
  const sym = Symbol("s");
  const source: any = { [sym]: 1, a: 2 };
  const output: any = Object.assign({}, source);
  return [output.a, output[sym], Object.getOwnPropertySymbols(output).length];
});
