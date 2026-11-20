import dns from "node:dns";
import dnsPromises from "node:dns/promises";

function shape(fn: () => unknown): string {
  try {
    fn();
    return "ok";
  } catch (error: any) {
    return `${error.name}/${error.code}`;
  }
}

for (
  const [label, resolver] of [
    ["callback", new dns.Resolver()],
    ["promises", new dnsPromises.Resolver()],
  ] as const
) {
  resolver.setServers(["127.0.0.1"]);
  const before = resolver.getServers().join("|");
  console.log(
    label + " not array:",
    shape(() => resolver.setServers("127.0.0.1" as any)),
  );
  console.log(
    label + " bad element:",
    shape(() => resolver.setServers([123] as any)),
  );
  console.log(label + " bad ip:", shape(() => resolver.setServers(["bad"])));
  console.log(
    label + " preserved:",
    before === resolver.getServers().join("|"),
  );
  console.log(
    label + " empty:",
    shape(() => resolver.setServers([])),
    resolver.getServers().length,
  );
}
