// URLSearchParams.prototype[Symbol.iterator] is an own prototype member and
// the same function as entries (WHATWG); code that checks for an iterator
// before iterating (Effect's UrlParams.fromInput) must find it.
const params = new URLSearchParams("a=1&b=x+y&a=3");
const proto = URLSearchParams.prototype as any;
console.log(typeof (params as any)[Symbol.iterator]);
console.log(proto[Symbol.iterator] === proto.entries);
const descriptor = Object.getOwnPropertyDescriptor(proto, Symbol.iterator)!;
console.log(descriptor.writable, descriptor.enumerable, descriptor.configurable);
const iterator = (params as any)[Symbol.iterator]();
const seen: string[] = [];
for (let step = iterator.next(); !step.done; step = iterator.next()) seen.push(step.value.join("="));
console.log(seen.join(","));
function entriesOf(input: any): Array<[string, string]> {
  return typeof input[Symbol.iterator] === "function" ? Array.from(input) : Object.entries(input);
}
console.log(JSON.stringify(entriesOf(params)));
// entries/keys/values return iterators driven by next(), and stay iterable.
const entries = params.entries();
const first = entries.next();
console.log(typeof entries.next, first.done, JSON.stringify(first.value));
console.log(JSON.stringify([...params.keys()]), JSON.stringify(Array.from(params.values())));
const pairs: string[] = [];
for (const [key, value] of params.entries()) pairs.push(key + ":" + value);
console.log(pairs.join(" "));
const keys = params.keys();
const steps: string[] = [];
for (let i = 0; i < 4; i++) {
  const step = keys.next();
  steps.push(String(step.done) + "/" + String(step.value));
}
console.log(steps.join(" "));
