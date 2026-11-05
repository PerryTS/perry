import dns from "node:dns";
import dnsPromises from "node:dns/promises";
import { startDnsServer } from "../fixtures/local-dns-server.mjs";

function callback4(options?: any): Promise<any> {
  return new Promise((resolve, reject) => {
    const done = (error: any, value: any) =>
      error ? reject(error) : resolve(value);
    options === undefined
      ? dns.resolve4("example.test", done)
      : dns.resolve4("example.test", options, done);
  });
}

function callback6(): Promise<any> {
  return new Promise((resolve, reject) => {
    dns.resolve6(
      "example.test",
      (error, value) => error ? reject(error) : resolve(value),
    );
  });
}

const server = await startDnsServer();
try {
  const address = `127.0.0.1:${server.port}`;
  dns.setServers([address]);
  dnsPromises.setServers([address]);
  console.log("callback A:", JSON.stringify(await callback4()));
  console.log(
    "callback A ttl:",
    JSON.stringify(await callback4({ ttl: true })),
  );
  console.log("callback AAAA:", JSON.stringify(await callback6()));
  console.log(
    "promise A:",
    JSON.stringify(await dnsPromises.resolve4("example.test")),
  );
  console.log(
    "promise A ttl:",
    JSON.stringify(await dnsPromises.resolve4("example.test", { ttl: true })),
  );
  console.log(
    "promise AAAA:",
    JSON.stringify(await dnsPromises.resolve6("example.test")),
  );
} finally {
  await server.close();
}
