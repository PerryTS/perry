import * as crypto from "node:crypto";
import { X509Certificate } from "node:crypto";

const caPem = `-----BEGIN CERTIFICATE-----
MIIDHzCCAgegAwIBAgIUQi4LftjxMHG8l96kJMCrdmH7dkIwDQYJKoZIhvcNAQEL
BQAwHzEdMBsGA1UEAwwUUGVycnkgQ2hlY2tJc3N1ZWQgQ0EwHhcNMjYwNjAzMjE1
OTA4WhcNMjYwNzAzMjE1OTA4WjAfMR0wGwYDVQQDDBRQZXJyeSBDaGVja0lzc3Vl
ZCBDQTCCASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBAJ2pr0DRJ7do6ng6
eAOkIybAGfatMgiXWcBfS3ZxO8+RHPR60EKn0lcxud/nHapvS8WRGapBYExUlOoL
nsuMDkvWUBqpqyy3OrZLOO+CQPMWmhY0xYrGllonHBE+3+pZfoBsEpXQ/7sUPvD5
I731f2mmYF2G7SXqTFDm9WNOJGXSLwLRCNjTSlNihUP627ZgK1X+i/nLgTLiQwqp
hsAtG1ujkxFwuvUsae8rIFAtogplJPj4UQPPezooXXECqINY5B32GRMREQL7aQYG
ZojY5V9GTDzQgeUudjbmeIBH9eL6ybI/D9cgtAygHSdvENP5CoMRT2f93bCRKKsP
TgiFw28CAwEAAaNTMFEwHQYDVR0OBBYEFBjxMj66h75tO2CGim8oUKv2akoMMB8G
A1UdIwQYMBaAFBjxMj66h75tO2CGim8oUKv2akoMMA8GA1UdEwEB/wQFMAMBAf8w
DQYJKoZIhvcNAQELBQADggEBAJqCiHK2WUuE0pp/uPlldyjX+WyRxAtxgbsJq1nr
n2qLF3ge/vB4O9jFo2AgTFP1hLQPi9CCIO6bdhEfKPZd+CVifH7Jnp/Vd7YLinEF
//FFnTWr7c2acyV0BceRihxyO+y6qSZ+PMrU1o+sIEuRcZ3uN+APNIJPt9gX0kub
uUIXdOx5MzT7kWRyAGsaGvvRL9sQzn07HtflVlqBvZzOctb6NqlOoe2Tbqa+9bri
2mjMobcGZCTQaAiVFjqTgv/XHeCebrCm4nyh9NJXGmZWvgvk8V8d3QMigGmOb+ly
MY9Q2hHYbEL2N1HqHf/GflaHo1f3KDJ3pAoVJGeWqR2vmN4=
-----END CERTIFICATE-----`;

const leafPem = `-----BEGIN CERTIFICATE-----
MIIDOTCCAiGgAwIBAgIUTl4ecABfreE+pEbhAEs/WhytnmUwDQYJKoZIhvcNAQEL
BQAwHzEdMBsGA1UEAwwUUGVycnkgQ2hlY2tJc3N1ZWQgQ0EwHhcNMjYwNjAzMjE1
OTA4WhcNMjYwNzAzMjE1OTA4WjAhMR8wHQYDVQQDDBZQZXJyeSBDaGVja0lzc3Vl
ZCBMZWFmMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAsFRGYmE16b5g
o8cZgI3YRwwL4fhMrFUHQZj4QYhghumHaLQL01doaJumZcfaopfhEGUvNgqMG03R
WfgJGavWy/i6HpNKXtpyd8N8Tmo2mBjHVu5oYAU7obcudHJCWN4JpPQkIjYbikHh
B/nBfv5dq5AFhqmo5kyc62MVgY8ZXWdGIJbCoRymzEZX4lY1ogFZscGGdSqZY3pB
DXD7OaVtDdLtpm1XGHpGs6ACZXc/QLXZt4ZVKr+AK3OoW6PGu1mVvJ2+tDdDVcJH
lnwQ8oUlGQXJQNIjMcEHqzdu7ul2RqxE3qJDLrehRR0i3eEcg7s5WAPQ5dlUm3cx
8NFt/SoWRQIDAQABo2swaTAJBgNVHRMEAjAAMBwGA1UdEQQVMBOCEWxlYWYuZXhh
bXBsZS50ZXN0MB0GA1UdDgQWBBQY6Fn3W6PnVu0YquyzvKWImxLjKDAfBgNVHSME
GDAWgBQY8TI+uoe+bTtghopvKFCr9mpKDDANBgkqhkiG9w0BAQsFAAOCAQEAFPlc
qWsMX4yXZ5n942QVCX6Uxr9yhu+8kJAPLnyISGJm/ojIbUh2DwmZvdy4Wc64U+mM
2UJvmYRiU+SDGTt6gD//x1l5L74Iz+0fAWKjvupmzOqw9sb5FOgCmcwsCN/z0Pwf
9kcCdlbdGXS+Y8yKkh+qrCMAj7FZfdrTDJrzSbtKmd+Fie4Zb94NytHrovrqXfey
rir8gdDVjpHP1Pyc/Avlj7+QWqYR2O/EICWzuPB0YSVrQVH1lQAFS8agO+E9VnUn
SHyGvXHoE4VIWi65wJG2pos8VGTrrVTOhUhpux1VCjO1FEWj/D344azquXG6r3lw
opbYuZtZ9lpEQiO/WA==
-----END CERTIFICATE-----`;

const ctor = crypto["X509Certificate"];
const proto = ctor["prototype"];
const desc = Object.getOwnPropertyDescriptor(proto, "issuerCertificate");
const leaf = new X509Certificate(leafPem);
const chainInput = new X509Certificate(`${leafPem}\n${caPem}`);

console.log("module ctor type:", typeof ctor);
console.log("prototype type:", typeof proto);
console.log(
  "prototype has issuerCertificate:",
  Object.getOwnPropertyNames(proto).includes("issuerCertificate"),
);
console.log("descriptor get type:", typeof desc?.get);
console.log("descriptor enumerable:", desc?.enumerable);
console.log("descriptor configurable:", desc?.configurable);
console.log("leaf issuer undefined:", leaf["issuerCertificate"] === undefined);
console.log("chain input issuer undefined:", chainInput["issuerCertificate"] === undefined);
