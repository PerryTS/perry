import dns from "node:dns";
import dnsPromises from "node:dns/promises";
import { startDnsServer } from "../fixtures/local-dns-server.mjs";

function shape(fn: () => unknown): string {
  try {
    const value = fn();
    return value instanceof Promise ? "promise" : typeof value;
  } catch (error: any) {
    return `${error.name}/${error.code}`;
  }
}

async function asyncShape(fn: () => Promise<unknown>): Promise<string> {
  try {
    await fn();
    return "resolved";
  } catch (error: any) {
    return `${error.name}/${error.code}`;
  }
}

const server = await startDnsServer();
try {
  const address = `127.0.0.1:${server.port}`;
  dns.setServers([address]);
  dnsPromises.setServers([address]);
  console.log(
    "callback:",
    shape(() =>
      dns.Resolver.prototype.resolve4.call({}, "example.test", () => {})
    ),
  );
  console.log(
    "promise:",
    await asyncShape(() =>
      dnsPromises.Resolver.prototype.resolve4.call({}, "example.test")
    ),
  );
} finally {
  await server.close();
}
