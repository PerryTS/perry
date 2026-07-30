import dns from "node:dns";
import dnsPromises from "node:dns/promises";

const callback = await new Promise<any>((resolve) => {
  dns.lookupService(
    "::1",
    0,
    (error, hostname, service) => resolve({ error, hostname, service }),
  );
});
console.log(
  "callback:",
  callback.error === null,
  typeof callback.hostname,
  typeof callback.service,
);

const promise = await dnsPromises.lookupService("::1", 0);
console.log("promise:", typeof promise.hostname, typeof promise.service);
