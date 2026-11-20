function errShape(err: any, fragment: string): string {
  const message = String(err?.message ?? "");
  return `${err?.name}:${message.includes(fragment)}:${err?.code ?? ""}`;
}

function bad(label: string, fragment: string, fn: () => unknown) {
  try {
    fn();
    console.log(`${label}:`, "NO_THROW");
  } catch (err: any) {
    console.log(`${label}:`, errShape(err, fragment));
  }
}

async function main() {
  const then = Promise.prototype.then;
  const catchFn = Promise.prototype.catch;
  const finallyFn = Promise.prototype.finally;

  const viaThen = then.call(Promise.resolve("ok"), (value: string) => `then:${value}`);
  console.log("then.return:", viaThen instanceof Promise, typeof viaThen?.then);
  console.log("then.value:", await viaThen);

  const viaApply = then.apply(Promise.resolve("apply"), [
    (value: string) => `apply:${value}`,
  ]);
  console.log("then.apply:", await viaApply);

  const passThrough = then.call(Promise.resolve("pass"), 7 as any);
  console.log("then.noncallable:", await passThrough);

  const viaCatch = catchFn.call(Promise.reject("boom"), (value: string) => `catch:${value}`);
  console.log("catch.return:", viaCatch instanceof Promise, typeof viaCatch?.then);
  console.log("catch.value:", await viaCatch);

  let cleanup = "no";
  const viaFinally = finallyFn.call(Promise.resolve("done"), () => {
    cleanup = "yes";
  });
  console.log("finally.return:", viaFinally instanceof Promise, typeof viaFinally?.then);
  console.log("finally.value:", await viaFinally, cleanup);

  const genericCatchReceiver = {
    then(onFulfilled: unknown, onRejected: unknown) {
      console.log("generic.catch.args:", typeof onFulfilled, typeof onRejected);
      return "generic-catch";
    },
  };
  console.log("generic.catch:", catchFn.call(genericCatchReceiver, () => "unused"));

  const genericFinallyReceiver = {
    then(onFulfilled: unknown, onRejected: unknown) {
      console.log("generic.finally.args:", typeof onFulfilled, typeof onRejected);
      return "generic-finally";
    },
  };
  console.log("generic.finally:", finallyFn.call(genericFinallyReceiver, () => "unused"));

  bad("bad.then.object", "Promise.prototype.then", () => then.call({}, () => "x"));
  bad("bad.then.null", "Promise.prototype.then", () => then.call(null as any, () => "x"));
  bad("bad.then.undefined", "Promise.prototype.then", () =>
    then.call(undefined as any, () => "x")
  );

  bad("bad.catch.object", "not a function", () => catchFn.call({}, () => "x"));
  bad("bad.catch.null", "Cannot read properties", () =>
    catchFn.call(null as any, () => "x")
  );
  bad("bad.catch.undefined", "Cannot read properties", () =>
    catchFn.call(undefined as any, () => "x")
  );

  bad("bad.finally.object", "not a function", () => finallyFn.call({}, () => "x"));
  bad("bad.finally.null", "non-object", () => finallyFn.call(null as any, () => "x"));
  bad("bad.finally.undefined", "non-object", () =>
    finallyFn.call(undefined as any, () => "x")
  );
}

await main();
