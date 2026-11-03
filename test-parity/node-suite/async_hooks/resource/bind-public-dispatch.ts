import { AsyncResource } from "node:async_hooks";

function probe(label: string, fn: () => unknown) {
  try {
    console.log(label, "ok", fn());
  } catch (error: any) {
    console.log(label, error.name, error.code || "no-code");
  }
}
const resource = new AsyncResource("ParityBindPublicDispatch");
const detachedBind = resource.bind;
const detached = detachedBind(() => "unused");
probe("detached implicit invocation", () => detached());
probe("detached explicit creation", () => detachedBind(() => "unused", null));

let reads = 0;
let replacement = false;
const dynamicThis = { tag: "dynamic" };
const fixedThis = { tag: "fixed" };
function callback(this: any, first: string, second: string) {
  return this.tag + ":" + first + ":" + second;
}
Object.defineProperty(callback, "length", { value: "public-length", configurable: true });
function dispatch(this: unknown, fn: Function, receiver: unknown, first: string, second: string) {
  console.log("dispatch identity", this === resource, fn === callback);
  return fn.call(receiver, first, second);
}
function replaced(this: unknown, fn: Function, receiver: unknown, first: string, second: string) {
  console.log("replacement identity", this === resource, fn === callback);
  return "replacement:" + fn.call(receiver, first, second);
}
Object.defineProperty(resource, "runInAsyncScope", {
  configurable: true,
  get() {
    reads++;
    return replacement ? replaced : dispatch;
  },
});
const implicit = resource.bind(callback);
console.log("implicit creation reads", reads);
console.log("implicit first", implicit.call(dynamicThis, "one", "two"));
console.log("implicit first reads", reads);
const explicit = resource.bind(callback, fixedThis);
console.log("explicit creation reads", reads);
replacement = true;
console.log("implicit replacement", implicit.call(dynamicThis, "three", "four"));
console.log("implicit replacement reads", reads);
console.log("explicit cached", explicit.call(dynamicThis, "five", "six"));
console.log("explicit cached reads", reads);
for (const bound of [implicit, explicit]) {
  const descriptor = Object.getOwnPropertyDescriptor(bound, "length")!;
  console.log("bound metadata", bound.name, bound.length, descriptor.value,
    descriptor.writable, descriptor.enumerable, descriptor.configurable);
}

let throwingReads = 0;
Object.defineProperty(resource, "runInAsyncScope", {
  configurable: true,
  get() {
    throwingReads++;
    throw new RangeError("public scope getter");
  },
});
probe("invalid callback first", () => resource.bind(1 as any, null));
console.log("invalid callback reads", throwingReads);
const lateThrow = resource.bind(callback, undefined);
console.log("throwing implicit creation reads", throwingReads);
probe("throwing implicit invocation", () => lateThrow());
console.log("throwing implicit invocation reads", throwingReads);
probe("throwing explicit creation", () => resource.bind(callback, false));
console.log("throwing explicit creation reads", throwingReads);
resource.emitDestroy();
