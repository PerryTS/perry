import dns from "node:dns";
import dnsPromises from "node:dns/promises";

function shape(fn: () => unknown): string {
  try {
    const result = fn();
    return result instanceof Promise ? "promise" : typeof result;
  } catch (error: any) {
    return `${error.name}/${error.code}`;
  }
}

const callback = new dns.Resolver();
const promises = new dnsPromises.Resolver();

for (
  const [label, fn] of [
    [
      "callback missing rrtype",
      () => (callback.resolve as any)("example.test"),
    ],
    [
      "callback bad rrtype",
      () => (callback.resolve as any)("example.test", "BAD", () => {}),
    ],
    [
      "callback rrtype type",
      () => (callback.resolve as any)("example.test", [], () => {}),
    ],
    [
      "callback missing name",
      () => (callback.resolve4 as any)(undefined, () => {}),
    ],
    [
      "callback missing callback",
      () => (callback.resolve4 as any)("example.test"),
    ],
    [
      "callback bad callback",
      () => (callback.resolve4 as any)("example.test", null),
    ],
  ] as const
) {
  console.log(label + ":", shape(fn));
}

async function promiseShape(fn: () => Promise<unknown>): Promise<string> {
  try {
    await fn();
    return "resolved";
  } catch (error: any) {
    return `${error.name}/${error.code}`;
  }
}

console.log(
  "promise bad rrtype:",
  await promiseShape(() => (promises.resolve as any)("example.test", "BAD")),
);
console.log(
  "promise rrtype type:",
  await promiseShape(() => (promises.resolve as any)("example.test", [])),
);
console.log(
  "promise missing name:",
  await promiseShape(() => (promises.resolve4 as any)()),
);
