import * as dns from "node:dns";
import * as dnsPromises from "node:dns/promises";

function callbackCall(fn: (cb: (err: any, value: any) => void) => void): Promise<any> {
  return new Promise((resolve) => {
    fn((err, value) => {
      resolve({ err, value });
    });
  });
}

function strings(value: unknown): boolean {
  return Array.isArray(value) && value.length > 0 && value.every((entry) => typeof entry === "string");
}

function thrownShape(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + ":", "no throw");
  } catch (e: any) {
    console.log(label + ":", e.name, e.code);
  }
}

const callback4 = await callbackCall((cb) => dns.resolve4("localhost", cb));
const callback6 = await callbackCall((cb) => dns.resolve6("localhost", cb));
const callbackA = await callbackCall((cb) => dns.resolve("localhost", "A", cb));
const callbackReverse = await callbackCall((cb) => dns.reverse("127.0.0.1", cb));
console.log("callback resolve4:", callback4.err === null, callback4.value.includes("127.0.0.1"));
console.log("callback resolve6:", callback6.err === null, callback6.value.includes("::1"));
console.log("callback resolve A:", callbackA.err === null, callbackA.value.includes("127.0.0.1"));
console.log("callback reverse:", callbackReverse.err === null, strings(callbackReverse.value));

const promise4 = await dnsPromises.resolve4("localhost");
const promiseReverse = await dnsPromises.reverse("127.0.0.1");
console.log("promise resolve4:", promise4.includes("127.0.0.1"));
console.log("promise reverse:", strings(promiseReverse));

const promiseResolver = new dnsPromises.Resolver();
const resolver4 = await promiseResolver.resolve4("localhost");
console.log("promise resolver resolve4:", resolver4.includes("127.0.0.1"));

thrownShape("callback bad rrtype", () => dns.resolve("localhost", "BAD", () => {}));
thrownShape("promise bad rrtype", () => dnsPromises.resolve("localhost", "BAD" as any));
