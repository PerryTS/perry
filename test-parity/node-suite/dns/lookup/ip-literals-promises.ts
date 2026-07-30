import dnsPromises from "node:dns/promises";

console.log("ipv4:", JSON.stringify(await dnsPromises.lookup("127.0.0.1")));
console.log("ipv6:", JSON.stringify(await dnsPromises.lookup("::1")));
console.log(
  "all:",
  JSON.stringify(await dnsPromises.lookup("127.0.0.1", { all: true })),
);
console.log("promise:", dnsPromises.lookup("127.0.0.1") instanceof Promise);
