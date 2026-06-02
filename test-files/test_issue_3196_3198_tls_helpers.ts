import * as tls from "node:tls";

function printErr(label, fn) {
  try {
    fn();
    console.log(label + ": ok");
  } catch (e) {
    const err = e;
    console.log(label + ":", err.name, err.code);
  }
}

const ciphers = tls.getCiphers();
console.log("tls helpers ciphers array:", Array.isArray(ciphers));
console.log("tls helpers ciphers first3:", ciphers.slice(0, 3).join(","));
console.log("tls helpers ciphers has tls13:", ciphers.includes("tls_aes_128_gcm_sha256"));

console.log("tls helpers default ca array:", Array.isArray(tls.getCACertificates()));
console.log("tls helpers bundled ca array:", Array.isArray(tls.getCACertificates("bundled")));
console.log("tls helpers extra ca length:", tls.getCACertificates("extra").length);
console.log("tls helpers root nonempty:", tls.rootCertificates.length > 0);
console.log(
  "tls constants:",
  tls.DEFAULT_ECDH_CURVE,
  tls.DEFAULT_MIN_VERSION,
  tls.DEFAULT_MAX_VERSION,
  tls.CLIENT_RENEG_LIMIT,
  tls.CLIENT_RENEG_WINDOW,
  typeof tls.DEFAULT_CIPHERS,
);

printErr("tls bad ca type", () => tls.getCACertificates("bad"));
printErr("tls bad default ca type", () => tls.setDefaultCACertificates("bad"));
printErr("tls bad default ca pem", () => tls.setDefaultCACertificates(["bad"]));
console.log("tls set empty ca:", tls.setDefaultCACertificates([]) === undefined);

const ctx = tls.createSecureContext({ minVersion: "TLSv1.2", maxVersion: "TLSv1.3" });
console.log(
  "tls ctx shape:",
  typeof ctx,
  ctx.constructor && ctx.constructor.name,
  "context" in ctx,
);
printErr("tls bad version", () => tls.createSecureContext({ minVersion: "SSLv3" }));

const dnsCert = { subjectaltname: "DNS:example.test", subject: { CN: "wrong.test" } };
console.log("tls dns ok:", tls.checkServerIdentity("example.test", dnsCert) === undefined);
const dnsMismatch = tls.checkServerIdentity("other.test", dnsCert);
console.log(
  "tls dns mismatch:",
  dnsMismatch.name,
  dnsMismatch.code,
  dnsMismatch.host,
  !!dnsMismatch.cert,
  typeof dnsMismatch.reason,
  dnsMismatch.reason.length > 0,
);

const wildCert = { subjectaltname: "DNS:*.example.test", subject: {} };
console.log("tls wildcard ok:", tls.checkServerIdentity("www.example.test", wildCert) === undefined);

const ipCert = { subjectaltname: "IP Address:127.0.0.1", subject: { CN: "wrong.test" } };
console.log("tls ip ok:", tls.checkServerIdentity("127.0.0.1", ipCert) === undefined);

const cnCert = { subject: { CN: "example.test" } };
console.log("tls cn ok:", tls.checkServerIdentity("example.test", cnCert) === undefined);

const numericHost = tls.checkServerIdentity(123, dnsCert);
console.log("tls numeric host:", numericHost.host, numericHost.code);
