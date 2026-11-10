import dns from "node:dns";
import dnsPromises from "node:dns/promises";
import { startDnsServer } from "../fixtures/local-dns-server.mjs";

function stable(value: any[]): string {
  return JSON.stringify(value.map((record) => {
    const copy = { ...record };
    if (copy.type !== "A" && copy.type !== "AAAA") delete copy.ttl;
    return Object.fromEntries(
      Object.keys(copy).sort().map((key) => [key, copy[key]]),
    );
  }));
}

const server = await startDnsServer();
try {
  const address = `127.0.0.1:${server.port}`;
  dns.setServers([address]);
  dnsPromises.setServers([address]);
  const callback: any[] = await new Promise((resolve, reject) => {
    dns.resolveAny(
      "example.test",
      (error, value) => error ? reject(error) : resolve(value),
    );
  });
  console.log("callback:", stable(callback));
  console.log("promise:", stable(await dnsPromises.resolveAny("example.test")));
} finally {
  await server.close();
}
