import dns from "node:dns";
import dnsPromises from "node:dns/promises";

const names = [
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
  "getServers",
  "setServers",
  "getDefaultResultOrder",
  "setDefaultResultOrder",
];

function metadata(value: any): string {
  return typeof value === "function"
    ? `${value.name}/${value.length}`
    : typeof value;
}

console.log(
  "callback:",
  names.map((name) => `${name}=${metadata((dns as any)[name])}`).join("|"),
);
console.log(
  "promises:",
  names.map((name) => `${name}=${metadata((dnsPromises as any)[name])}`).join(
    "|",
  ),
);
