// `new dns.Resolver()` and `new dns.promises.Resolver()` on a namespace read at
// runtime (CommonJS `require`), and on a constructor read off the namespace as a
// value (`const { Resolver } = require('node:dns/promises')`). The ESM form is
// bound at compile time; the runtime namespace built no resolver at all.
// Hermetic: no query is sent, only the server list is read and written.
// The CommonJS helper holds the literal `require` calls (see test_gap_dns_cjs_namespace).
import helper from "./gap_dns_cjs_namespace_helper.cjs";
const { dns, dnsPromises } = helper;

const callback = new dns.Resolver();
callback.setServers(["1.1.1.1"]);
console.log("dns.Resolver", typeof callback, typeof callback.resolve4, callback.getServers());

const promises = new dns.promises.Resolver();
promises.setServers(["8.8.8.8"]);
console.log("dns.promises.Resolver", typeof promises, typeof promises.resolve4, promises.getServers());

const Ctor = dnsPromises.Resolver;
const viaValue = new Ctor({ timeout: 1000 });
viaValue.setServers(["9.9.9.9"]);
console.log("dns/promises Resolver", typeof Ctor, typeof viaValue.resolve4, viaValue.getServers());

console.log("independent", callback.getServers().join(), promises.getServers().join(), viaValue.getServers().join());
