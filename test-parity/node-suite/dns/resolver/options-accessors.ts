import dns from "node:dns";
import dnsPromises from "node:dns/promises";

function run(Resolver: any): string {
  const log: string[] = [];
  const prototype = {
    get timeout() {
      log.push("timeout");
      return -1;
    },
    get tries() {
      log.push("tries");
      return 4;
    },
    get maxTimeout() {
      log.push("maxTimeout");
      return 0;
    },
  };
  new Resolver(Object.create(prototype));
  return log.join("|");
}

console.log("callback:", run(dns.Resolver));
console.log("promises:", run(dnsPromises.Resolver));
