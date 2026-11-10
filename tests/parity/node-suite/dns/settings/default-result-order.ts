import * as dns from "node:dns";
import * as dnsPromises from "node:dns/promises";

function thrownShape(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + ":", "no throw");
  } catch (e: any) {
    console.log(label + ":", e.name, e.code);
  }
}

function getPromiseOrder(): string {
  return typeof dnsPromises.getDefaultResultOrder === "function"
    ? dnsPromises.getDefaultResultOrder()
    : "absent";
}

console.log("initial:", dns.getDefaultResultOrder(), getPromiseOrder());

dns.setDefaultResultOrder("ipv4first");
console.log("callback set:", dns.getDefaultResultOrder(), getPromiseOrder());

if (typeof dnsPromises.setDefaultResultOrder === "function") {
  dnsPromises.setDefaultResultOrder("ipv6first");
}
console.log("promise set:", dns.getDefaultResultOrder(), getPromiseOrder());

dns.setDefaultResultOrder("verbatim");
console.log("verbatim:", dns.getDefaultResultOrder(), getPromiseOrder());

thrownShape("invalid order", () => dns.setDefaultResultOrder("bad" as any));
console.log("order preserved:", dns.getDefaultResultOrder());
