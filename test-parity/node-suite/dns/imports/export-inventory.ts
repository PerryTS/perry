import * as dns from "node:dns";
import * as dnsPromises from "node:dns/promises";

console.log("dns keys:", Object.keys(dns).sort().join("|"));
console.log("promise keys:", Object.keys(dnsPromises).sort().join("|"));
console.log("default keys:", Object.keys(dns.default).sort().join("|"));
console.log(
  "promise default keys:",
  Object.keys(dnsPromises.default).sort().join("|"),
);
