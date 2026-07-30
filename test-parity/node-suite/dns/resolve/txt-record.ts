import dns from "node:dns";
import dnsPromises from "node:dns/promises";
import { startDnsServer } from "../fixtures/local-dns-server.mjs";

const server = await startDnsServer();
try {
  const address = `127.0.0.1:${server.port}`;
  dns.setServers([address]);
  dnsPromises.setServers([address]);
  const callback = await new Promise((resolve, reject) => {
    dns.resolveTxt(
      "example.test",
      (error, value) => error ? reject(error) : resolve(value),
    );
  });
  console.log("callback:", JSON.stringify(callback));
  console.log(
    "promise:",
    JSON.stringify(await dnsPromises.resolveTxt("example.test")),
  );
} finally {
  await server.close();
}
