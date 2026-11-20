import dns from "node:dns";
import dnsPromises from "node:dns/promises";

const callbackBefore = dns.resolve4;
const promiseBefore = dnsPromises.resolve4;

function promiseServers(): string {
  return typeof dnsPromises.getServers === "function"
    ? dnsPromises.getServers().join("|")
    : "absent";
}

dns.setServers(["127.0.0.1:5301"]);
console.log("callback rebound:", callbackBefore !== dns.resolve4);
console.log(
  "promise rebound by callback:",
  promiseBefore !== dnsPromises.resolve4,
);
console.log(
  "shared callback set:",
  dns.getServers().join("|"),
  promiseServers(),
);

const callbackAfter = dns.resolve4;
const promiseAfter = dnsPromises.resolve4;
if (typeof dnsPromises.setServers === "function") {
  dnsPromises.setServers(["127.0.0.1:5302"]);
}
console.log("callback rebound by promise:", callbackAfter !== dns.resolve4);
console.log("promise rebound:", promiseAfter !== dnsPromises.resolve4);
console.log("promise-only set:", dns.getServers().join("|"), promiseServers());
