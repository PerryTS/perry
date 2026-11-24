import * as http2 from "node:http2";

// `http2.connect`'s `servername` option is the name the client sends as SNI and
// checks the server's certificate against, in place of the authority's host
// (PerryTS/perry#15313). The client dials `127.0.0.1`; the certificate names
// only `api.local`, so the session connects only if `servername` is used.
// The certificate is `tests/parity/node-suite/tls/fixtures/api-local-cert.pem`
// (SAN DNS:api.local, valid to 2036) and its key is `localhost-key.pem`,
// inlined so the case is self-contained.
const cert = `-----BEGIN CERTIFICATE-----
MIIDHzCCAgegAwIBAgIUJp6us08mvPkb7xXvvgYpDBR5F3owDQYJKoZIhvcNAQEL
BQAwFDESMBAGA1UEAwwJYXBpLmxvY2FsMB4XDTI2MDcxNjEwMTg0M1oXDTM2MDcx
MzEwMTg0M1owFDESMBAGA1UEAwwJYXBpLmxvY2FsMIIBIjANBgkqhkiG9w0BAQEF
AAOCAQ8AMIIBCgKCAQEAjekpyhiK0q4H8TQo01JTA564FZpOitgwvIYMe3qhf0dF
lo2CbjxJcx5GOQ57k6vcNlLfIL2yV8f7hJNuFlfLAFvtm9pm45BvbsPvduW1AuSI
3oA/fpfsQ5K1VgAPbLZFhdndCjoGW3/ZO8PbUC5DTge5luCfXoV0zFzZATbJxziy
QZ9nYspc58Se6Xj0KhM3XCy2S7V5wVPRXo2nIW5ho83yHfKyVyKEew7nloxhrNAY
iRwHzVzBvCotdgZK/lBm1qsugHs31LR6T75izQGooIN1wz2V9kiHCW+s3CmgFCy2
5AbN547xLwn4djh5Tz4lJhA4Rh0D0F/vzceL5ToJBwIDAQABo2kwZzAdBgNVHQ4E
FgQU1s+brNmcdkCqkncnW6rNlJpdiP0wHwYDVR0jBBgwFoAU1s+brNmcdkCqkncn
W6rNlJpdiP0wDwYDVR0TAQH/BAUwAwEB/zAUBgNVHREEDTALgglhcGkubG9jYWww
DQYJKoZIhvcNAQELBQADggEBACe3jZjwb9a0yZvHMBUTOThRxH0AVRfHqr41IamP
lZf9nOMokjTUAPp5+5FkIkIsyase72mm8fCmWrpDDT9SdiIpHY7b7N2ryJNmVA1c
sIdnexJedazm4F/DPGGxEb7TLIcanSRocyFuuFVclCEy7P0hCiQzoyPOPelRP8JN
XT5DRzvmd6dg+BU7PS6/WRdk8LsaZ3lnN1zry49SGnS3ibrZX00oWjVyPcd0nJUa
TItZKt+YXFg0loZMsIXkAVw4Jan9RZ3Nh0RHWf1MCDNloI4IlqSGkeOevpIuJQzW
7F/+Ti8/2S7wCCBE2v2ZvzxuwSFkDOSbcrzfMLh5E8YTajY=
-----END CERTIFICATE-----
`;
const key = `-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCN6SnKGIrSrgfx
NCjTUlMDnrgVmk6K2DC8hgx7eqF/R0WWjYJuPElzHkY5DnuTq9w2Ut8gvbJXx/uE
k24WV8sAW+2b2mbjkG9uw+925bUC5IjegD9+l+xDkrVWAA9stkWF2d0KOgZbf9k7
w9tQLkNOB7mW4J9ehXTMXNkBNsnHOLJBn2diylznxJ7pePQqEzdcLLZLtXnBU9Fe
jachbmGjzfId8rJXIoR7DueWjGGs0BiJHAfNXMG8Ki12Bkr+UGbWqy6AezfUtHpP
vmLNAaigg3XDPZX2SIcJb6zcKaAULLbkBs3njvEvCfh2OHlPPiUmEDhGHQPQX+/N
x4vlOgkHAgMBAAECggEACFfV8iDBQKOkqeSkJdBoOwVA01xQE8+kBeFnqHbMOdxp
1fEZ4vs+Yjs8a6xTTZpEBxmWLqmYa5rBSckVJtEgiTPeY1RSyjw6oOt6D6Zvnuzq
sxIdKYcrB8n/SUAVqBGLQtRNL4W7y/NXRTE9mpgtss+3dIxeMkNsW3t18qFS+Zhg
TP8q984k+zl3QOz6sc5T39Unuk1g98LC2sjCXwKANzZRMBMigoGDnWgk9t86cEXM
YWmyStS89HKEDmxWQMRIc/6zw5YC9Jo0cF2OJxGtN/O+LLeeNoJcdnlSAcyYRU1Q
asJhtNkMwfMRrTVH0kQfF5X3a/aJfusiJnQBcvlVeQKBgQDF8rI8dlaQ7jNOfSoZ
FhphZe1DriFaulRA9PUwrxEb/qvRstre0Egu967ILmqoqKfufyNT4W5JnWngliN6
S7D9cvxpW0RsUQHZXMqZp7s6kt4hAdziuyC2Wx2y6+zFHkbOwJcaULYrSNHJCPOj
cMu5TIplum+hnO9rMHKEpE0fAwKBgQC3h17rEy4uFbWPQD3fNjAi9QzIKX9wm8eD
SYekgZaHpAjrLCa8oNR6qMxU5Cpn7I3o2HegSUe29jDAr8GMp47JYRTGMHUl1Zwa
KtSGEH19sRhVUqIVW2h2/tysuaYpK1hFjPWM+KpKQFNzgt2EPf5057zE7gOHLcAL
UccMgP1crQKBgQCy4h1SaHrYZHq3LoNRwli6thrRc9YuoH4taXD+uuaSTvZE/gWv
H7hrwWcQ/mli229PJ1PspKc/HWMmE2giR669jCEwsMrHu/kYzjNE4oBfcYQNfhp4
RzVLtlHDdFM226KPixnCLThDK35x14YdqHxiixnyzqW8/g6a5mBHIBeVswKBgQCT
y79DndGdqTvqHbj1zWScci0V8F1BqSHVd1x1vSolF5NbF9YmJ3qVQOQ0JP6FbHmn
ntNPUFQhYkdGlQNQKwuQ3s5lAFcG3ev1IrK9OABnPTu0UnRWsKMC2SGLM4I9Ozu9
3tNL8GDqpLzPk/6h5W7KZGifSnGq5cv3EaczSZk/jQKBgAcaLGi25ozeFgK1qvuQ
WFTjLYV6KaMrGd5+NF+2a/NQsDGTZSF1egKUvE5QH5YNf37xWkqwvR3rsbenxLAG
aNYjvX+bUs4Mc/bgNkO51P9sH6YoKsuFzTTx4eR5ZS+dtfoiZMfzKkRBK4Baggrv
7S9Q3thVBhvBcz19oFN2Rmvf
-----END PRIVATE KEY-----
`;

const server = http2.createSecureServer({ key, cert });
server.on("stream", (stream) => {
  stream.respond({ ":status": 200 });
  stream.end("ok");
});
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const port = (server.address() as any).port;
const events: string[] = [];
await new Promise<void>((resolve) => {
  const client = http2.connect(`https://127.0.0.1:${port}`, { ca: cert, servername: "api.local" });
  client.on("connect", () => events.push("connect"));
  client.on("error", (error: any) => events.push(`error code=${error.code}`));
  client.on("close", () => {
    events.push("close");
    resolve();
  });
  const request = client.request({ ":path": "/" });
  request.on("response", (headers: any) => events.push(`status=${headers[":status"]}`));
  request.on("error", () => {});
  request.on("end", () => client.close());
  request.resume();
});
console.log(events.join("\n"));
server.close();
