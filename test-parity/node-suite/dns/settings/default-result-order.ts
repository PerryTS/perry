import * as dns from "node:dns";
import * as dnsPromises from "node:dns/promises";

function lookupCb(hostname: string): Promise<any> {
  return new Promise((resolve) => {
    dns.lookup(hostname, (err, address, family) => {
      resolve({ err, address, family });
    });
  });
}

function orderSummary(values: Array<{ address: string; family: number }>): string {
  return values.map((value) => `${value.family}:${value.address}`).join("|");
}

function thrownShape(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + ":", "no throw");
  } catch (e: any) {
    console.log(label + ":", e.name, e.code);
  }
}

dns.setDefaultResultOrder("ipv4first");
const callback4 = await lookupCb("localhost");
const promise4 = await dnsPromises.lookup("localhost");
const all4 = await dnsPromises.lookup("localhost", { all: true });
console.log("ipv4 callback:", callback4.err === null, callback4.address, callback4.family);
console.log("ipv4 promise:", promise4.address, promise4.family);
console.log("ipv4 all:", orderSummary(all4));

dnsPromises.setDefaultResultOrder("ipv6first");
const callback6 = await lookupCb("localhost");
const promise6 = await dnsPromises.lookup("localhost");
const all6 = await dnsPromises.lookup("localhost", { all: true });
console.log("ipv6 callback:", callback6.err === null, callback6.address, callback6.family);
console.log("ipv6 promise:", promise6.address, promise6.family);
console.log("ipv6 all:", orderSummary(all6));
console.log("shared order:", dns.getDefaultResultOrder(), dnsPromises.getDefaultResultOrder());

thrownShape("invalid order", () => dns.setDefaultResultOrder("bad" as any));
console.log("order preserved:", dns.getDefaultResultOrder());
