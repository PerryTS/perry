import dns from "node:dns";
import dnsPromises from "node:dns/promises";

function shape(fn: () => unknown): string {
  try {
    const value = fn();
    return value instanceof Promise ? "promise" : typeof value;
  } catch (error: any) {
    return `${error.name}/${error.code}`;
  }
}

async function promiseShape(value: Promise<unknown>): Promise<string> {
  try {
    await value;
    return "resolved";
  } catch (error: any) {
    return `${error.name}/${error.code}`;
  }
}

console.log(
  "callback address:",
  shape(() => dns.reverse("not-an-ip", () => {})),
);
console.log(
  "callback missing:",
  shape(() => (dns.reverse as any)("127.0.0.1")),
);
console.log(
  "callback callback:",
  shape(() => (dns.reverse as any)("127.0.0.1", null)),
);
console.log(
  "promise address:",
  await promiseShape(dnsPromises.reverse("not-an-ip")),
);
