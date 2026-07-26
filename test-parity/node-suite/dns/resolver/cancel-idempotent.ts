import dns from "node:dns";
import dnsPromises from "node:dns/promises";

for (
  const [label, resolver] of [
    ["callback", new dns.Resolver()],
    ["promises", new dnsPromises.Resolver()],
  ] as const
) {
  resolver.setServers(["127.0.0.1:5300"]);
  console.log(
    label + ":",
    resolver.cancel(),
    resolver.cancel(),
    resolver.getServers().length > 0,
  );
}
