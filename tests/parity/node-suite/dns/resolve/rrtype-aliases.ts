import dns from "node:dns";
import dnsPromises from "node:dns/promises";
import { startDnsServer } from "../fixtures/local-dns-server.mjs";

function callback(
  call: (done: (error: any, value: any) => void) => unknown,
): Promise<any> {
  return new Promise((resolve, reject) => {
    const returned = call((error, value) =>
      error ? reject(error) : resolve(value)
    );
    console.log("callback return:", typeof returned, returned !== null);
  });
}

const server = await startDnsServer();
try {
  const address = `127.0.0.1:${server.port}`;
  dns.setServers([address]);
  dnsPromises.setServers([address]);
  console.log(
    "default callback:",
    JSON.stringify(await callback((done) => dns.resolve("example.test", done))),
  );
  console.log(
    "A callback:",
    JSON.stringify(
      await callback((done) => dns.resolve("example.test", "A", done)),
    ),
  );
  console.log(
    "default promise:",
    JSON.stringify(await dnsPromises.resolve("example.test")),
  );
  console.log(
    "A promise:",
    JSON.stringify(await dnsPromises.resolve("example.test", "A")),
  );
} finally {
  await server.close();
}
