function showError(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(label + ":ok:" + String(value));
  } catch (error: any) {
    console.log(label + ":throw:" + error.name);
  }
}

const then = Promise.prototype.then;
const catchMethod = Promise.prototype.catch;
const finallyMethod = Promise.prototype.finally;

console.log("types:", typeof then, typeof catchMethod, typeof finallyMethod);
console.log("names:", then.name, catchMethod.name, finallyMethod.name);
console.log("lengths:", then.length, catchMethod.length, finallyMethod.length);

showError("then object", () => then.call({}));
showError("then undefined", () => then.call(undefined));
showError("then null", () => then.call(null));
showError("then number", () => then.call(1));
showError("then apply object", () => then.apply({}, []));
showError("then detached bare", () => then(() => "x"));
showError("catch object", () => catchMethod.call({}));
showError("catch undefined", () => catchMethod.call(undefined));
showError("catch detached bare", () => catchMethod(() => "x"));
showError("finally object", () => finallyMethod.call({}));
showError("finally undefined", () => finallyMethod.call(undefined));
showError("finally detached bare", () => finallyMethod(() => "x"));

then.call(Promise.resolve("ok"), (value) => console.log("then valid:" + value));
then.apply(Promise.resolve("apply"), [(value) => console.log("then apply valid:" + value)]);
catchMethod.call(Promise.reject(new TypeError("caught")), (error) =>
  console.log("catch valid:" + error.name)
);
finallyMethod.call(Promise.resolve("done"), () => console.log("finally callback"))
  .then((value) => console.log("finally valid:" + value));

await Promise.resolve();
await Promise.resolve();
