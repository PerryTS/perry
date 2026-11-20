import dns from "node:dns";
import dnsPromises from "node:dns/promises";
import { startDnsServer } from "../fixtures/local-dns-server.mjs";

function callback(
  call: (done: (error: any, value: any) => void) => void,
): Promise<any> {
  return new Promise((resolve, reject) =>
    call((error, value) => error ? reject(error) : resolve(value))
  );
}

const server = await startDnsServer();
try {
  const address = `127.0.0.1:${server.port}`;
  dns.setServers([address]);
  dnsPromises.setServers([address]);
  console.log(
    "callback resolveCname:",
    JSON.stringify(
      await callback((done) => dns.resolveCname("example.test", done)),
    ),
  );
  console.log(
    "promise resolveCname:",
    JSON.stringify(await dnsPromises.resolveCname("example.test")),
  );
  console.log(
    "callback resolveNs:",
    JSON.stringify(
      await callback((done) => dns.resolveNs("example.test", done)),
    ),
  );
  console.log(
    "promise resolveNs:",
    JSON.stringify(await dnsPromises.resolveNs("example.test")),
  );
  console.log(
    "callback resolvePtr:",
    JSON.stringify(
      await callback((done) => dns.resolvePtr("example.test", done)),
    ),
  );
  console.log(
    "promise resolvePtr:",
    JSON.stringify(await dnsPromises.resolvePtr("example.test")),
  );
} finally {
  await server.close();
}
