import tls from "node:tls";

const ciphers = tls.getCiphers();
console.log("getCiphers function:", typeof tls.getCiphers === "function");
console.log("ciphers array:", Array.isArray(ciphers) && ciphers.length >= 10);
console.log("ciphers sorted:", ciphers[0] === "aes128-gcm-sha256" &&
  ciphers[1] === "aes128-sha" &&
  ciphers[ciphers.length - 1] === "tls_chacha20_poly1305_sha256");
console.log("ciphers known:", ciphers[0] === "aes128-gcm-sha256" && ciphers.includes("tls_aes_256_gcm_sha384"));

console.log("default constants:", tls.DEFAULT_ECDH_CURVE === "auto" &&
  tls.DEFAULT_MIN_VERSION === "TLSv1.2" &&
  tls.DEFAULT_MAX_VERSION === "TLSv1.3" &&
  typeof tls.DEFAULT_CIPHERS === "string" &&
  tls.DEFAULT_CIPHERS.length > 0 &&
  tls.CLIENT_RENEG_LIMIT === 3 &&
  tls.CLIENT_RENEG_WINDOW === 600);

console.log("root certs:", Array.isArray(tls.rootCertificates) &&
  Object.isFrozen(tls.rootCertificates) &&
  tls.rootCertificates.length > 0 &&
  typeof tls.rootCertificates[0] === "string" &&
  tls.rootCertificates[0].startsWith("-----BEGIN CERTIFICATE-----"));

for (const type of ["default", "system", "bundled", "extra"] as const) {
  const certs = tls.getCACertificates(type);
  const markerOk = type === "extra" || certs[0]?.startsWith("-----BEGIN CERTIFICATE-----") === true;
  console.log(`ca ${type}:`, Array.isArray(certs) && Object.isFrozen(certs) && markerOk);
}

console.log("ca default arg:", tls.getCACertificates().length === tls.getCACertificates("default").length);

try {
  tls.getCACertificates("invalid" as any);
  console.log("ca invalid value: no throw");
} catch (err: any) {
  console.log("ca invalid value:", err instanceof TypeError, err.code);
}

try {
  tls.getCACertificates(1 as any);
  console.log("ca invalid type: no throw");
} catch (err: any) {
  console.log("ca invalid type:", err instanceof TypeError, err.code);
}

console.log("set empty default ca:", tls.setDefaultCACertificates([]) === undefined &&
  tls.getCACertificates("default").length === 0);

const firstRoot = tls.getCACertificates("bundled")[0];
console.log("set custom default ca:", tls.setDefaultCACertificates([firstRoot]) === undefined &&
  tls.getCACertificates("default").length === 1 &&
  tls.getCACertificates("default")[0]?.startsWith("-----BEGIN CERTIFICATE-----") === true);

try {
  tls.setDefaultCACertificates("bad" as any);
  console.log("set ca non-array: no throw");
} catch (err: any) {
  console.log("set ca non-array:", err instanceof TypeError, err.code);
}

try {
  tls.setDefaultCACertificates(["bad pem"]);
  console.log("set ca invalid pem: no throw");
} catch (err: any) {
  console.log("set ca invalid pem:", err instanceof Error, err.code);
}
