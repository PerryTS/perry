import dns from "node:dns";
import dnsPromises from "node:dns/promises";

const methods = [
  "resolve",
  "resolve4",
  "resolve6",
  "resolveAny",
  "resolveCaa",
  "resolveCname",
  "resolveMx",
  "resolveNaptr",
  "resolveNs",
  "resolvePtr",
  "resolveSoa",
  "resolveSrv",
  "resolveTlsa",
  "resolveTxt",
  "reverse",
  "cancel",
  "getServers",
  "setServers",
  "setLocalAddress",
];

function metadata(prototype: any): string {
  return methods.map((name) => {
    const value = prototype[name];
    return `${name}=${
      typeof value === "function"
        ? `${value.name}/${value.length}`
        : typeof value
    }`;
  }).join("|");
}

console.log("callback:", metadata(dns.Resolver.prototype));
console.log("promises:", metadata(dnsPromises.Resolver.prototype));
