import dns from "node:dns";
import dnsPromises from "node:dns/promises";

function thrownShape(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + ":", "no throw");
  } catch (e: any) {
    console.log(label + ":", e.name, e.code);
  }
}

dns.setServers(["8.8.8.8", "[2001:4860:4860::8888]:53", "1.1.1.1:5353", "[2001:4860:4860::8844]:5353"]);
dnsPromises.setServers(["9.9.9.9"]);
console.log("callback servers:", dns.getServers().join("|"));
console.log("promise servers:", dnsPromises.getServers().join("|"));

const resolverA = new dns.Resolver();
const resolverB = new dns.Resolver();
resolverA.setServers(["4.4.4.4"]);
resolverB.setServers(["[2001:db8::1]:5353"]);
console.log("resolver servers:", resolverA.getServers().join("|"), resolverB.getServers().join("|"));
console.log("module servers unchanged:", dns.getServers().join("|"));

const promiseResolver = new dnsPromises.Resolver();
promiseResolver.setServers(["5.5.5.5"]);
console.log("promise resolver servers:", promiseResolver.getServers().join("|"), dnsPromises.getServers().join("|"));
console.log("cancel returns:", resolverA.cancel(), promiseResolver.cancel());

thrownShape("invalid not array", () => dns.setServers("8.8.8.8" as any));
thrownShape("invalid element", () => dns.setServers([123] as any));
thrownShape("invalid ip", () => dns.setServers(["bad"] as any));
