// A spread's yield belongs to the enclosing generator, even though HIR
// object construction uses an immediately invoked closure.
const parameters = { ast: "schema" };
function* initialize() {
  yield "init";
  return { description: "tool", parameters };
}
function wrap(init: any) {
  return function* () {
    return typeof init === "function" ? { ...(yield* init()) } : { ...init };
  };
}
const delegated = wrap(initialize)();
console.log(JSON.stringify(delegated.next()));
console.log(JSON.stringify(delegated.next()));
console.log(JSON.stringify(wrap({ parameters })().next()));

const events: string[] = [];
function mark(value: string) { events.push(value); return value; }
function* ordered() {
  return {
    before: mark("before"),
    ...(yield "spread"),
    [yield "key"]: yield "value",
    after: mark("after"),
    method() { return "method"; },
  };
}
const iterator = ordered();
console.log(JSON.stringify(iterator.next()), events.join(","));
const source = { get parameters() { mark("get"); return parameters; } };
console.log(JSON.stringify(iterator.next(source)), events.join(","));
console.log(JSON.stringify(iterator.next("slot")), events.join(","));
const result = iterator.next(42);
console.log(result.done, JSON.stringify(result.value), result.value.method(), events.join(","));
