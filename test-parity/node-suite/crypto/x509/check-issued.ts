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

const otherPem = `-----BEGIN CERTIFICATE-----
MIIDJTCCAg2gAwIBAgIUDBgkXCbbRWU2zy0qDM/I0Oiq+RMwDQYJKoZIhvcNAQEL
BQAwIjEgMB4GA1UEAwwXUGVycnkgQ2hlY2tJc3N1ZWQgT3RoZXIwHhcNMjYwNjAz
MjE1OTA4WhcNMjYwNzAzMjE1OTA4WjAiMSAwHgYDVQQDDBdQZXJyeSBDaGVja0lz
c3VlZCBPdGhlcjCCASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBAK41DqfH
yQ1YRNL9YT0QXLMxTkH8njuAVfHMQIJJouClyK4Bc6lnUhnjGxZh1HRZaCeaw4J6
GTOWFLp0mB7YtDvZuiIlc0rDrZWwWza51BQ/Iiv0N5jOo2OAa68E2Wa0hhsS0Mgt
S455C7wgwQldL9DFbukqgpa9w99jSrB5ACO0zSMo8lSEivDH1sv5v3NLazB/vc3c
ZKF4gspIP9IG4R6dTLePsrlVniYMGHozZ0C9b1kwyXK2S8dkKbIg9PIe8asxMiao
FOT0ICpivL9vyA/FhAfkHhmutA4rMn3iaTOMm5TfMrg4I87LjmnDphnRxX/7XZmF
ne1k+H0j6sWMgocCAwEAAaNTMFEwHQYDVR0OBBYEFLfswwqBNB0gkdP323wNKOG2
MGn/MB8GA1UdIwQYMBaAFLfswwqBNB0gkdP323wNKOG2MGn/MA8GA1UdEwEB/wQF
MAMBAf8wDQYJKoZIhvcNAQELBQADggEBADeWDZ/zwCmEo/Ygt7ZgWSVuO9hJXgDe
Zdea+jAlCOjOaK4+0fZGOnqygSiOMTLYrSECqYVHD69nEUFt2fi3hvzMz6tGTAJf
kXUvSszhJ2UIKg0kiYDESh31ITaa4SZHEa3vCe9C6rLbTIVCP7C37pDx5a2PqjSO
S3426Zk1BzEE+KlzX/Hm/W1It3ksPd/FUI267E5CWbiMNiAqfSN23aPTOireXm61
buIOpVetw9H6FIC9B/lRzbDcOMQs37Gv4XPO8V4gxFrg2x3gpwIAYIdPcAm9eocu
FjHBbd4lY/WjIKqVM9Pl+qo/4zY3puhe58Mu18URB8EKkSIjbsy3LAI=
-----END CERTIFICATE-----`;

const sameSubjectPem = `-----BEGIN CERTIFICATE-----
MIIDHzCCAgegAwIBAgIUa7SBI0jI5P05I1xvb0BcUG7sIIAwDQYJKoZIhvcNAQEL
BQAwHzEdMBsGA1UEAwwUUGVycnkgQ2hlY2tJc3N1ZWQgQ0EwHhcNMjYwNjAzMjE1
OTI5WhcNMjYwNzAzMjE1OTI5WjAfMR0wGwYDVQQDDBRQZXJyeSBDaGVja0lzc3Vl
ZCBDQTCCASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBAJ+DtArETPvkXsAR
QvaU1Pucr2edMNGRl2hcnIkVKsMuZWSakz0f0bWCfjvWkbEn7bo2S0ge6S04i7aa
5v9f0lasHnDJon2p+baShW7tIZomW7iR7NfFmSn/ve7KHIo6e9S+kWkmdhL2aP3N
EVcwS46jHYGzf1CkpdMw1AYE2Pg+AvBBQkqwKFCiUtndnhF5hJcIXYOHgHgj2ODN
8jFKlXQ+I9vHTLmo9Z1K253yuqejODtwEJf9cSDIWN0ThluS/blwB1p/2WEj+aAK
LCj6gj5ZXVIiRWwlcIPpdVZkNd/zkUdvhxAKGWITCcZLrXz8WKnoCrGnFAlukjkm
q+QibpkCAwEAAaNTMFEwHQYDVR0OBBYEFO7fegjhiDHaCKJ8tsQq1iiKzM16MB8G
A1UdIwQYMBaAFO7fegjhiDHaCKJ8tsQq1iiKzM16MA8GA1UdEwEB/wQFMAMBAf8w
DQYJKoZIhvcNAQELBQADggEBAD01tYjbKarpqeJjMEK/fSmHOJm1WIKNZBg3BREq
Mo4MDmMd0OIKZgC8pyTfHrAnp6eCtJ3r4P0weFyHsOhKZQxijy7KXBJWXWEnei/8
xZ+Pg90iSJj4QLijpBrTOSeewXBFuSCSjiq5LwoPPW/C45dhaGVkp2/SEZ3iiWFc
Q4b/ifZX9LEOHh0zx1TtpQGwSS+oKuozFojxHL0WqEQiyAxQq0ksv9lRqpZBkGfy
T7EJP4HEFUnlV5p1Sr8tlG1PeLOnXNQtIFt05B9ujOklQPFGoeudj3pMs+YPEC8l
9TROOIf7UZ0ph3Mwq+n98b1dD7YmInT2mQevK8oTr8Mze5g=
-----END CERTIFICATE-----`;

const ca = new X509Certificate(caPem);
const leaf = new X509Certificate(leafPem);
const other = new X509Certificate(otherPem);
const sameSubject = new X509Certificate(sameSubjectPem);

function report(label: string, fn: () => unknown) {
  try {
    console.log(`${label}:`, fn());
  } catch (err) {
    const e = err as Error & { code?: string };
    console.log(
      `${label}: err`,
      e.constructor.name,
      e.code || "",
      e.message.includes("X509Certificate"),
    );
  }
}

console.log("typeof checkIssued:", typeof leaf["checkIssued"]);
report("leaf issued by ca", () => leaf["checkIssued"](ca));
report("leaf issued by leaf", () => leaf["checkIssued"](leaf));
report("ca self issued", () => ca["checkIssued"](ca));
report("ca issued by leaf", () => ca["checkIssued"](leaf));
report("leaf issued by other", () => leaf["checkIssued"](other));
report("leaf issued by same subject", () => leaf["checkIssued"](sameSubject));
report("other self issued", () => other["checkIssued"](other));
report("string arg", () => leaf["checkIssued"]("bad" as any));
