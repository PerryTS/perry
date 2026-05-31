import dns from "node:dns";
import dnsPromises from "node:dns/promises";

// --- #3336: constants + error aliases ---
console.log(typeof dns.ADDRCONFIG, dns.ADDRCONFIG);
console.log(typeof dns.V4MAPPED, dns.V4MAPPED);
console.log(typeof dns.ALL, dns.ALL);
console.log(typeof dns.NODATA, dns.NODATA);
console.log(typeof dns.FORMERR, dns.FORMERR);
console.log(typeof dns.SERVFAIL, dns.SERVFAIL);
console.log(typeof dns.NOTFOUND, dns.NOTFOUND);
console.log(typeof dns.REFUSED, dns.REFUSED);
console.log(typeof dns.TIMEOUT, dns.TIMEOUT);
console.log(typeof dns.CANCELLED, dns.CANCELLED);
console.log(typeof dnsPromises.NODATA, dnsPromises.NODATA);
console.log(typeof dnsPromises.NOTFOUND, dnsPromises.NOTFOUND);
console.log(typeof dnsPromises.CANCELLED, dnsPromises.CANCELLED);

// --- #3162: lookup / lookupService. Wrap callbacks in promises so the whole
// sequence is a single deterministic await chain (no microtask-vs-immediate
// ordering ambiguity across runtimes). All inputs are loopback → deterministic.
function lookupCb(host: string, opts: any): Promise<string> {
  return new Promise((resolve) => {
    dns.lookup(host, opts, (err: any, address: string, family: number) => {
      resolve(`${err} ${address} ${family}`);
    });
  });
}
function lookupServiceCb(addr: string, port: number): Promise<string> {
  return new Promise((resolve) => {
    dns.lookupService(addr, port, (err: any, hostname: string, service: string) => {
      resolve(`${err} ${hostname} ${service}`);
    });
  });
}

(async () => {
  console.log("cb-ip", await lookupCb("127.0.0.1", {}));
  console.log("cb-fam4", await lookupCb("localhost", { family: 4 }));
  console.log("cb-svc80", await lookupServiceCb("127.0.0.1", 80));
  console.log("cb-svc443", await lookupServiceCb("127.0.0.1", 443));

  const r = await dnsPromises.lookup("127.0.0.1");
  console.log("p-ip", r.address, r.family);
  const r4 = await dnsPromises.lookup("localhost", { family: 4 });
  console.log("p-fam4", r4.address, r4.family);
  const svc = await dnsPromises.lookupService("127.0.0.1", 80);
  console.log("p-svc", svc.hostname, svc.service);
})();
