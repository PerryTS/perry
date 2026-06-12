import * as dns from "node:dns";
import * as dnsPromises from "node:dns/promises";

// resolve* answers depend on the machine's configured nameserver (some
// resolvers answer A/AAAA for "localhost", some return ENODATA/NXDOMAIN), so
// every check prints either the matched record or the error code instead of
// assuming success — node and Perry must agree either way.

function callbackCall(fn: (cb: (err: any, value: any) => void) => void): Promise<any> {
  return new Promise((resolve) => {
    fn((err, value) => {
      resolve({ err, value });
    });
  });
}

function summarize(err: any, value: unknown, expected: string): string {
  if (err) return "err:" + err.code;
  if (!Array.isArray(value)) return "value:" + typeof value;
  return value.includes(expected) ? "has " + expected : JSON.stringify(value);
}

function reverseSummary(err: any, value: unknown): string {
  if (err) return "err:" + err.code;
  return JSON.stringify(value);
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
console.log("callback resolve4:", summarize(callback4.err, callback4.value, "127.0.0.1"));
console.log("callback resolve6:", summarize(callback6.err, callback6.value, "::1"));
console.log("callback resolve A:", summarize(callbackA.err, callbackA.value, "127.0.0.1"));
console.log("callback reverse:", reverseSummary(callbackReverse.err, callbackReverse.value));

let promise4: string;
try {
  promise4 = summarize(null, await dnsPromises.resolve4("localhost"), "127.0.0.1");
} catch (e: any) {
  promise4 = "err:" + e.code;
}
console.log("promise resolve4:", promise4);

let promiseReverse: string;
try {
  promiseReverse = reverseSummary(null, await dnsPromises.reverse("127.0.0.1"));
} catch (e: any) {
  promiseReverse = "err:" + e.code;
}
console.log("promise reverse:", promiseReverse);

const promiseResolver = new dnsPromises.Resolver();
let resolver4: string;
try {
  resolver4 = summarize(null, await promiseResolver.resolve4("localhost"), "127.0.0.1");
} catch (e: any) {
  resolver4 = "err:" + e.code;
}
console.log("promise resolver resolve4:", resolver4);

thrownShape("callback bad rrtype", () => dns.resolve("localhost", "BAD", () => {}));
thrownShape("promise bad rrtype", () => dnsPromises.resolve("localhost", "BAD" as any));
