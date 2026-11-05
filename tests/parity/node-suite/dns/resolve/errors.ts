import dns from "node:dns";
import dnsPromises from "node:dns/promises";
import { startDnsServer } from "../fixtures/local-dns-server.mjs";

function summary(error: any): string {
  return `${error.name}/${error.code}/${error.syscall}/${error.hostname}/${
    Object.keys(error).sort().join("|")
  }`;
}

for (const mode of ["nxdomain", "nodata", "refused"] as const) {
  const server = await startDnsServer(mode);
  try {
    const address = `127.0.0.1:${server.port}`;
    dns.setServers([address]);
    dnsPromises.setServers([address]);
    const callback = await new Promise<string>((resolve) => {
      dns.resolve4(`${mode}.example.test`, (error) => resolve(summary(error)));
    });
    const promise = await dnsPromises.resolve4(`${mode}.example.test`).then(
      () => "resolved",
      (error) => summary(error),
    );
    console.log(mode + ":", callback, promise);
  } finally {
    await server.close();
  }
}
