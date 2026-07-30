import dns from "node:dns";
import dnsPromises from "node:dns/promises";
import { startDnsServer } from "../fixtures/local-dns-server.mjs";

const server = await startDnsServer("silent");
try {
  const address = `127.0.0.1:${server.port}`;
  dns.setServers([address]);
  if (typeof dnsPromises.setServers === "function") {
    dnsPromises.setServers([address]);
  }
  const callbackResolver = new dns.Resolver();
  callbackResolver.setServers([address]);
  const callbackResult = new Promise<string>((resolve) => {
    callbackResolver.resolve4("callback.example.test", (error) => {
      resolve(`${error?.name}/${error?.code}/${error?.syscall}`);
    });
  });
  await server.nextQuery();
  console.log(
    "callback cancel:",
    callbackResolver.cancel(),
    callbackResolver.cancel(),
  );
  console.log("callback result:", await callbackResult);

  const promiseResolver = new dnsPromises.Resolver();
  promiseResolver.setServers([address]);
  const promiseResult = promiseResolver.resolve4("promise.example.test").catch((
    error,
  ) => `${error.name}/${error.code}/${error.syscall}`);
  await server.nextQuery();
  console.log(
    "promises cancel:",
    promiseResolver.cancel(),
    promiseResolver.cancel(),
  );
  console.log("promises result:", await promiseResult);
} finally {
  await server.close();
}
