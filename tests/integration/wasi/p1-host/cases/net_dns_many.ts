// A name with more addresses than one read of the host's answer holds: every
// one of them comes back.
import * as dns from "node:dns";

dns.lookup("many.test", { all: true }, (error: any, addresses: any[]) => {
  const last = addresses[addresses.length - 1];
  console.log(error, addresses.length, addresses[0].address, last.address);
});
