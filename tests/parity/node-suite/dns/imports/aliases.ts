import dns from "node:dns";
import dnsPromises from "node:dns/promises";

const methods = [
  "lookup",
  "lookupService",
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
];

console.log("promises identity:", dns.promises === dnsPromises);
console.log("resolver identity:", dns.Resolver === dnsPromises.Resolver);
console.log(
  "method identity:",
  methods.map((name) => (dns as any)[name] === (dnsPromises as any)[name]).join(
    "|",
  ),
);
console.log(
  "error identity:",
  dns.NODATA === dnsPromises.NODATA,
  dns.CANCELLED === dnsPromises.CANCELLED,
);
