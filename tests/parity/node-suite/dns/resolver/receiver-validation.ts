import dns from "node:dns";
import dnsPromises from "node:dns/promises";

function shape(fn: () => unknown): string {
  try {
    fn();
    return "no throw";
  } catch (error: any) {
    return `${error.name}/${error.code}`;
  }
}

for (
  const [label, prototype] of [
    ["callback", dns.Resolver.prototype],
    ["promises", dnsPromises.Resolver.prototype],
  ] as const
) {
  console.log(label + " cancel:", shape(() => prototype.cancel.call({})));
  console.log(
    label + " getServers:",
    shape(() => prototype.getServers.call({})),
  );
  console.log(
    label + " setServers:",
    shape(() => prototype.setServers.call({}, ["127.0.0.1"])),
  );
  console.log(
    label + " setLocalAddress:",
    shape(() => prototype.setLocalAddress.call({}, "127.0.0.1")),
  );
}
