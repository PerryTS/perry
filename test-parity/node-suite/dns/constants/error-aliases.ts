import { NODATA as namedNodata } from "node:dns";
import { CANCELLED as namedPromisesCancelled } from "node:dns/promises";
import * as dns from "node:dns";
import * as dnsPromises from "node:dns/promises";

console.log("dns.ADDRCONFIG:", typeof dns.ADDRCONFIG, dns.ADDRCONFIG);
console.log("dns.V4MAPPED:", typeof dns.V4MAPPED, dns.V4MAPPED);
console.log("dns.ALL:", typeof dns.ALL, dns.ALL);
console.log("dnsPromises.ADDRCONFIG:", typeof dnsPromises.ADDRCONFIG);
console.log("dnsPromises.V4MAPPED:", typeof dnsPromises.V4MAPPED);
console.log("dnsPromises.ALL:", typeof dnsPromises.ALL);
console.log("dns.promises.NODATA:", typeof dns.promises.NODATA, dns.promises.NODATA);

console.log("dns.NODATA:", typeof dns.NODATA, dns.NODATA);
console.log("dns.FORMERR:", typeof dns.FORMERR, dns.FORMERR);
console.log("dns.SERVFAIL:", typeof dns.SERVFAIL, dns.SERVFAIL);
console.log("dns.NOTFOUND:", typeof dns.NOTFOUND, dns.NOTFOUND);
console.log("dns.NOTIMP:", typeof dns.NOTIMP, dns.NOTIMP);
console.log("dns.REFUSED:", typeof dns.REFUSED, dns.REFUSED);
console.log("dns.BADQUERY:", typeof dns.BADQUERY, dns.BADQUERY);
console.log("dns.BADNAME:", typeof dns.BADNAME, dns.BADNAME);
console.log("dns.BADFAMILY:", typeof dns.BADFAMILY, dns.BADFAMILY);
console.log("dns.BADRESP:", typeof dns.BADRESP, dns.BADRESP);
console.log("dns.CONNREFUSED:", typeof dns.CONNREFUSED, dns.CONNREFUSED);
console.log("dns.TIMEOUT:", typeof dns.TIMEOUT, dns.TIMEOUT);
console.log("dns.EOF:", typeof dns.EOF, dns.EOF);
console.log("dns.FILE:", typeof dns.FILE, dns.FILE);
console.log("dns.NOMEM:", typeof dns.NOMEM, dns.NOMEM);
console.log("dns.DESTRUCTION:", typeof dns.DESTRUCTION, dns.DESTRUCTION);
console.log("dns.BADSTR:", typeof dns.BADSTR, dns.BADSTR);
console.log("dns.BADFLAGS:", typeof dns.BADFLAGS, dns.BADFLAGS);
console.log("dns.NONAME:", typeof dns.NONAME, dns.NONAME);
console.log("dns.BADHINTS:", typeof dns.BADHINTS, dns.BADHINTS);
console.log("dns.NOTINITIALIZED:", typeof dns.NOTINITIALIZED, dns.NOTINITIALIZED);
console.log("dns.LOADIPHLPAPI:", typeof dns.LOADIPHLPAPI, dns.LOADIPHLPAPI);
console.log(
  "dns.ADDRGETNETWORKPARAMS:",
  typeof dns.ADDRGETNETWORKPARAMS,
  dns.ADDRGETNETWORKPARAMS,
);
console.log("dns.CANCELLED:", typeof dns.CANCELLED, dns.CANCELLED);

console.log("dnsPromises.NODATA:", typeof dnsPromises.NODATA, dnsPromises.NODATA);
console.log("dnsPromises.FORMERR:", typeof dnsPromises.FORMERR, dnsPromises.FORMERR);
console.log("dnsPromises.SERVFAIL:", typeof dnsPromises.SERVFAIL, dnsPromises.SERVFAIL);
console.log("dnsPromises.NOTFOUND:", typeof dnsPromises.NOTFOUND, dnsPromises.NOTFOUND);
console.log("dnsPromises.NOTIMP:", typeof dnsPromises.NOTIMP, dnsPromises.NOTIMP);
console.log("dnsPromises.REFUSED:", typeof dnsPromises.REFUSED, dnsPromises.REFUSED);
console.log("dnsPromises.BADQUERY:", typeof dnsPromises.BADQUERY, dnsPromises.BADQUERY);
console.log("dnsPromises.BADNAME:", typeof dnsPromises.BADNAME, dnsPromises.BADNAME);
console.log("dnsPromises.BADFAMILY:", typeof dnsPromises.BADFAMILY, dnsPromises.BADFAMILY);
console.log("dnsPromises.BADRESP:", typeof dnsPromises.BADRESP, dnsPromises.BADRESP);
console.log(
  "dnsPromises.CONNREFUSED:",
  typeof dnsPromises.CONNREFUSED,
  dnsPromises.CONNREFUSED,
);
console.log("dnsPromises.TIMEOUT:", typeof dnsPromises.TIMEOUT, dnsPromises.TIMEOUT);
console.log("dnsPromises.EOF:", typeof dnsPromises.EOF, dnsPromises.EOF);
console.log("dnsPromises.FILE:", typeof dnsPromises.FILE, dnsPromises.FILE);
console.log("dnsPromises.NOMEM:", typeof dnsPromises.NOMEM, dnsPromises.NOMEM);
console.log(
  "dnsPromises.DESTRUCTION:",
  typeof dnsPromises.DESTRUCTION,
  dnsPromises.DESTRUCTION,
);
console.log("dnsPromises.BADSTR:", typeof dnsPromises.BADSTR, dnsPromises.BADSTR);
console.log("dnsPromises.BADFLAGS:", typeof dnsPromises.BADFLAGS, dnsPromises.BADFLAGS);
console.log("dnsPromises.NONAME:", typeof dnsPromises.NONAME, dnsPromises.NONAME);
console.log("dnsPromises.BADHINTS:", typeof dnsPromises.BADHINTS, dnsPromises.BADHINTS);
console.log(
  "dnsPromises.NOTINITIALIZED:",
  typeof dnsPromises.NOTINITIALIZED,
  dnsPromises.NOTINITIALIZED,
);
console.log(
  "dnsPromises.LOADIPHLPAPI:",
  typeof dnsPromises.LOADIPHLPAPI,
  dnsPromises.LOADIPHLPAPI,
);
console.log(
  "dnsPromises.ADDRGETNETWORKPARAMS:",
  typeof dnsPromises.ADDRGETNETWORKPARAMS,
  dnsPromises.ADDRGETNETWORKPARAMS,
);
console.log("dnsPromises.CANCELLED:", typeof dnsPromises.CANCELLED, dnsPromises.CANCELLED);

console.log("named aliases:", namedNodata, namedPromisesCancelled);
