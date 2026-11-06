import tls from "node:tls";

const implicit = tls.getCACertificates();
const explicit = tls.getCACertificates("default");
console.log("default identity:", implicit === explicit, explicit === tls.getCACertificates("default"));
console.log("bundled identity:", tls.getCACertificates("bundled") === tls.rootCertificates);
for (const type of ["default", "bundled", "system", "extra"] as const) {
  const first = tls.getCACertificates(type);
  const second = tls.getCACertificates(type);
  console.log(type + ":", first === second, Object.isFrozen(first));
}
