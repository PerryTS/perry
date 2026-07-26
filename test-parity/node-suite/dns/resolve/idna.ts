import dns from "node:dns";
import dnsPromises from "node:dns/promises";
import { startDnsServer } from "../fixtures/local-dns-server.mjs";

const server = await startDnsServer("idna");
try {
  const address = `127.0.0.1:${server.port}`;
  dns.setServers([address]);
  dnsPromises.setServers([address]);
  const callback = await new Promise((resolve) => {
    dns.resolve4(
      "mañana.example",
      (error, value) =>
        resolve(error ? `error:${error.code}` : JSON.stringify(value)),
    );
  });
  console.log("callback:", callback);
  const promise = await dnsPromises.resolve4("mañana.example").then(
    (value) => JSON.stringify(value),
    (error) => `error:${error.code}`,
  );
  console.log("promise:", promise);
} finally {
  await server.close();
}
