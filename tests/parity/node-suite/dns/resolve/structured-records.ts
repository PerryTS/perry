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
    "callback resolveCaa:",
    JSON.stringify(
      await callback((done) => dns.resolveCaa("example.test", done)),
    ),
  );
  console.log(
    "promise resolveCaa:",
    JSON.stringify(await dnsPromises.resolveCaa("example.test")),
  );
  console.log(
    "callback resolveMx:",
    JSON.stringify(
      await callback((done) => dns.resolveMx("example.test", done)),
    ),
  );
  console.log(
    "promise resolveMx:",
    JSON.stringify(await dnsPromises.resolveMx("example.test")),
  );
  console.log(
    "callback resolveNaptr:",
    JSON.stringify(
      await callback((done) => dns.resolveNaptr("example.test", done)),
    ),
  );
  console.log(
    "promise resolveNaptr:",
    JSON.stringify(await dnsPromises.resolveNaptr("example.test")),
  );
  console.log(
    "callback resolveSoa:",
    JSON.stringify(
      await callback((done) => dns.resolveSoa("example.test", done)),
    ),
  );
  console.log(
    "promise resolveSoa:",
    JSON.stringify(await dnsPromises.resolveSoa("example.test")),
  );
  console.log(
    "callback resolveSrv:",
    JSON.stringify(
      await callback((done) => dns.resolveSrv("example.test", done)),
    ),
  );
  console.log(
    "promise resolveSrv:",
    JSON.stringify(await dnsPromises.resolveSrv("example.test")),
  );
} finally {
  await server.close();
}
