import dnsDefault from "node:dns";
import * as dnsNs from "node:dns";
import dnsPromisesDefault from "node:dns/promises";
import * as dnsPromisesNs from "node:dns/promises";
import process from "node:process";

const dnsBuiltin = process.getBuiltinModule("dns");
const dnsPromisesBuiltin = process.getBuiltinModule("dns/promises");

console.log("dns namespace has default:", Object.keys(dnsNs).includes("default"));
console.log("dns default identity:", dnsNs.default === dnsDefault);
console.log("dns builtin identity:", dnsBuiltin === dnsDefault);
console.log("dns default lacks default key:", !Object.keys(dnsDefault).includes("default"));
console.log("dns lookup types:", typeof dnsDefault.lookup, typeof dnsNs.lookup);
console.log("dns lookup identity:", dnsDefault.lookup === dnsNs.lookup);

console.log("promises namespace has default:", Object.keys(dnsPromisesNs).includes("default"));
console.log("promises default identity:", dnsPromisesNs.default === dnsPromisesDefault);
console.log("promises builtin identity:", dnsPromisesBuiltin === dnsPromisesDefault);
console.log(
  "promises default lacks default key:",
  !Object.keys(dnsPromisesDefault).includes("default"),
);
console.log(
  "promises Resolver types:",
  typeof dnsPromisesDefault.Resolver,
  typeof dnsPromisesNs.Resolver,
);

const resolver = new dnsPromisesDefault.Resolver();
// resolve4 queries the configured nameserver directly (it does not read
// /etc/hosts), so "localhost" yields an A record on some resolvers and
// ESERVFAIL/ENOTFOUND on others — node itself rejects (and aborts on the
// unhandled rejection) when the resolver has no answer for it. Print
// array-or-error-code so node and Perry agree regardless of the machine's
// resolver, the same record-or-error approach the dns/resolve suite uses.
let resolver4Summary: string;
try {
  const resolver4 = await resolver.resolve4("localhost");
  resolver4Summary = Array.isArray(resolver4) ? "array" : typeof resolver4;
} catch (e: any) {
  resolver4Summary = "err:" + e.code;
}
console.log("promises default resolve4:", resolver4Summary);
