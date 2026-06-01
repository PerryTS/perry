const globalFetch = globalThis.fetch;
const rebound = fetch;
const indexed = globalThis["fetch"];
const descriptor = Object.getOwnPropertyDescriptor(globalThis, "fetch");

console.log("typeof fetch:", typeof fetch);
console.log("typeof globalThis.fetch:", typeof globalFetch);
console.log("identity:", fetch === globalFetch, rebound === globalFetch, indexed === globalFetch);
console.log("name length:", globalFetch.name, globalFetch.length);
console.log(
  "descriptor:",
  !!descriptor,
  descriptor?.writable,
  descriptor?.enumerable,
  descriptor?.configurable,
  descriptor?.value === globalFetch,
);
console.log("rebound name length:", rebound.name, rebound.length);

const replacement = function fetchReplacement(input: unknown) {
  return `mock:${String(input)}`;
};

(globalThis as any).fetch = replacement;
console.log(
  "reassigned:",
  globalThis.fetch === replacement,
  fetch === replacement,
  globalThis["fetch"] === replacement,
  (fetch as any)("url"),
);
