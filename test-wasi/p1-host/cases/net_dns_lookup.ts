// dns.lookup asks the host to resolve: the answer comes after the host's delay,
// and an unknown name is ENOTFOUND.
import * as dns from "node:dns";

const started = Date.now();
dns.lookup("lookup.test", (error: any, address: string, family: number) => {
  console.log(error, address, family, Date.now() - started >= 250 ? "after delay" : "too early");
  dns.lookup("gone.test", (error: any) => console.log(error.code));
});
