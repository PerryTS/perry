import dns from "node:dns";
import dnsPromises from "node:dns/promises";
import { promisify } from "node:util";

const lookup = promisify(dns.lookup);
const lookupService = promisify(dns.lookupService);

console.log("lookup result:", JSON.stringify(await lookup("127.0.0.1")));
const service: any = await lookupService("127.0.0.1", 0);
console.log(
  "service keys:",
  Object.keys(service).sort().join("|"),
  typeof service.hostname,
  typeof service.service,
);
console.log(
  "custom identity:",
  (dns.lookup as any)[promisify.custom] === dnsPromises.lookup,
);
console.log(
  "custom args enumerable:",
  Object.getOwnPropertyDescriptor(
    dns.lookup,
    Symbol.for("nodejs.util.promisify.customArgs"),
  )?.enumerable,
);
