import dns from "node:dns";
import dnsPromises from "node:dns/promises";

function ownNames(value: object): string {
  return Object.getOwnPropertyNames(value).sort().join("|");
}

console.log("callback prototype:", ownNames(dns.Resolver.prototype));
console.log("promise prototype:", ownNames(dnsPromises.Resolver.prototype));
console.log(
  "callback base:",
  ownNames(Object.getPrototypeOf(dns.Resolver.prototype)),
);
console.log(
  "promise base:",
  ownNames(Object.getPrototypeOf(dnsPromises.Resolver.prototype)),
);
console.log(
  "constructors:",
  new dns.Resolver() instanceof dns.Resolver,
  new dnsPromises.Resolver() instanceof dnsPromises.Resolver,
);
