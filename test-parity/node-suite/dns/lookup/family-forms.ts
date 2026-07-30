import dns from "node:dns";
import dnsPromises from "node:dns/promises";

function callbackFamily(family: any): Promise<string> {
  return new Promise((resolve) => {
    dns.lookup("127.0.0.1", { family }, (error, address, resultFamily) => {
      resolve(
        error ? `${error.name}/${error.code}` : `${address}/${resultFamily}`,
      );
    });
  });
}

function syncShape(fn: () => unknown): string {
  try {
    const value = fn();
    return value instanceof Promise ? "promise" : typeof value;
  } catch (error: any) {
    return `${error.name}/${error.code}`;
  }
}

console.log("callback IPv4:", await callbackFamily("IPv4"));
console.log("callback IPv6 mismatch:", await callbackFamily("IPv6"));
console.log(
  "promise IPv4:",
  syncShape(() => dnsPromises.lookup("127.0.0.1", { family: "IPv4" as any })),
);
console.log("negative zero:", await callbackFamily(-0));
