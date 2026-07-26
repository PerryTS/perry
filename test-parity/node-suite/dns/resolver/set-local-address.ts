import dns from "node:dns";
import dnsPromises from "node:dns/promises";

function shape(fn: () => unknown): string {
  try {
    return "ok/" + String(fn());
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
  console.log(
    label + " ipv4:",
    shape(() => resolver.setLocalAddress("127.0.0.1")),
  );
  console.log(label + " ipv6:", shape(() => resolver.setLocalAddress("::1")));
  console.log(
    label + " pair:",
    shape(() => resolver.setLocalAddress("127.0.0.1", "::1")),
  );
  console.log(
    label + " wrong pair:",
    shape(() => resolver.setLocalAddress("127.0.0.1", "127.0.0.1")),
  );
  console.log(label + " bad:", shape(() => resolver.setLocalAddress("bad")));
  console.log(
    label + " missing:",
    shape(() => (resolver.setLocalAddress as any)()),
  );
}
