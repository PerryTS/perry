import * as dns from "node:dns";
import * as dnsPromises from "node:dns/promises";

function isLoopback(address: unknown): boolean {
  return address === "127.0.0.1" || address === "::1";
}

function lookupCb(hostname: string, options?: unknown): Promise<any> {
  return new Promise((resolve) => {
    const cb = (err: any, value: any, family?: any) => resolve({ err, value, family });
    if (options === undefined) dns.lookup(hostname, cb);
    else dns.lookup(hostname, options as any, cb);
  });
}

function lookupServiceCb(address: string, port: number): Promise<any> {
  return new Promise((resolve) => {
    dns.lookupService(address, port, (err: any, hostname: any, service: any) => {
      resolve({ err, hostname, service });
    });
  });
}

function thrownShape(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + ":", "no throw");
  } catch (e: any) {
    console.log(label + ":", e.name, e.code);
  }
}

const one = await lookupCb("localhost");
console.log("callback lookup loopback:", one.err === null, isLoopback(one.value), one.family === 4 || one.family === 6);

const all = await lookupCb("localhost", { all: true });
console.log("callback lookup all:", all.err === null, Array.isArray(all.value), all.value.every((entry: any) => isLoopback(entry.address)));

const family4 = await lookupCb("localhost", { family: 4 });
console.log("callback lookup family4:", family4.err === null, family4.value === "127.0.0.1", family4.family);

const service = await lookupServiceCb("127.0.0.1", 80);
console.log("callback lookupService:", service.err === null, typeof service.hostname, service.service);

const promiseOne = await dnsPromises.lookup("localhost");
console.log("promise lookup loopback:", isLoopback(promiseOne.address), promiseOne.family === 4 || promiseOne.family === 6);

const promiseAll = await dnsPromises.lookup("localhost", { all: true });
console.log("promise lookup all:", Array.isArray(promiseAll), promiseAll.every((entry: any) => isLoopback(entry.address)));

const promiseFamily4 = await dnsPromises.lookup("localhost", { family: 4 });
console.log("promise lookup family4:", promiseFamily4.address === "127.0.0.1", promiseFamily4.family);

const promiseService = await dnsPromises.lookupService("127.0.0.1", 80);
console.log("promise lookupService:", typeof promiseService.hostname, promiseService.service);

thrownShape("callback lookup missing callback", () => dns.lookup("localhost" as any));
thrownShape("callback lookup invalid family", () => dns.lookup("localhost", { family: 5 } as any, () => {}));
thrownShape("callback lookupService bad port", () => dns.lookupService("127.0.0.1", -1, () => {}));
thrownShape("promise lookup invalid family", () => dnsPromises.lookup("localhost", { family: 5 } as any));
thrownShape("promise lookupService bad port", () => dnsPromises.lookupService("127.0.0.1", -1));
