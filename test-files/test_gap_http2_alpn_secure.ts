// @covers node:http2 createSecureServer ALPN negotiation and h2 over TLS (#10327)
//
// ORACLE: Node 26.5.1. Perry's `http2.connect()` is cleartext-only (#10327),
// and the secure server side has never been measured against a real TLS peer,
// so every line here is new ground. The certificate and key are the repo's
// existing `test-parity/node-suite/tls/fixtures/localhost-{cert,key}.pem`,
// inlined so the fixture is self-contained (CN=localhost, SAN DNS:localhost +
// IP:127.0.0.1, valid to 2036).
//
// Established against Node:
//   * `createSecureServer()` offers ONLY "h2": a TLS client that offers only
//     "http/1.1" is rejected during the handshake with
//     ERR_SSL_TLSV1_ALERT_NO_APPLICATION_PROTOCOL — not a fallback, a failure;
//   * `createSecureServer({ allowHTTP1: true })` negotiates "http/1.1" for
//     such a client and still prefers "h2" when both are offered;
//   * a client that offers NO ALPN protocols gets `socket.alpnProtocol === false`
//     (the boolean, not undefined and not a string);
//   * an end-to-end h2-over-TLS request works and the session's socket reports
//     `alpnProtocol === "h2"`;
//   * with `allowHTTP1: true`, an HTTP/1.1 client reaches the 'request'
//     handler with `req.httpVersion === "1.1"`.
import http2 from "node:http2";
import { Buffer } from "node:buffer";
import tls from "node:tls";
import https from "node:https";
import { barrier, waitEvent, withTimeout } from "./_helpers/h2_wire.ts";

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
-----END PRIVATE KEY-----`;

const cert = `-----BEGIN CERTIFICATE-----
MIIDJTCCAg2gAwIBAgIUZF3wbyk6BduDu+lEeegKd2ULMK8wDQYJKoZIhvcNAQEL
BQAwFDESMBAGA1UEAwwJbG9jYWxob3N0MB4XDTI2MDUyNDE3NDI1NloXDTM2MDUy
MTE3NDI1NlowFDESMBAGA1UEAwwJbG9jYWxob3N0MIIBIjANBgkqhkiG9w0BAQEF
AAOCAQ8AMIIBCgKCAQEAjekpyhiK0q4H8TQo01JTA564FZpOitgwvIYMe3qhf0dF
lo2CbjxJcx5GOQ57k6vcNlLfIL2yV8f7hJNuFlfLAFvtm9pm45BvbsPvduW1AuSI
3oA/fpfsQ5K1VgAPbLZFhdndCjoGW3/ZO8PbUC5DTge5luCfXoV0zFzZATbJxziy
QZ9nYspc58Se6Xj0KhM3XCy2S7V5wVPRXo2nIW5ho83yHfKyVyKEew7nloxhrNAY
iRwHzVzBvCotdgZK/lBm1qsugHs31LR6T75izQGooIN1wz2V9kiHCW+s3CmgFCy2
5AbN547xLwn4djh5Tz4lJhA4Rh0D0F/vzceL5ToJBwIDAQABo28wbTAdBgNVHQ4E
FgQU1s+brNmcdkCqkncnW6rNlJpdiP0wHwYDVR0jBBgwFoAU1s+brNmcdkCqkncn
W6rNlJpdiP0wDwYDVR0TAQH/BAUwAwEB/zAaBgNVHREEEzARgglsb2NhbGhvc3SH
BH8AAAEwDQYJKoZIhvcNAQELBQADggEBAHFmvSxFCTHcqiocEHF3i0seBmNwWq40
TtyVf9qyZYUZVqM/Z7tGDsNfNOhM+YscLs1ZTs8XzdpdYBEVyCLDYGjb4Cv6r5gS
hr+E0NQBnPuker6Rw64nzahfWYjf/Eo+7nwUbCahTbXHAs43c4m0bmL02r1NxVmv
BKGQKO/uR9Dy+3TKykNQkacKJ6oDxdTDovMUKlbwU/HlyzwK/HTm762cJfgZiMYM
uru8x9wmqogCQSAz2q6a6q/CZfn1o7S5KiWd0FzinP+50g5cSL/ob0GJ8Jge1oI5
5rap/3DFfnTn0zfJ60U52+BVFnOIqkYT7/g5N4laGrza73tYXq7FV4s=
-----END CERTIFICATE-----`;

function sleep(ms: number): Promise<void> {
  return new Promise<void>((resolve) => setTimeout(resolve, ms));
}

async function alpnProbe(label: string, options: any, offered: string[]): Promise<void> {
  const server: any = http2.createSecureServer(Object.assign({ key: key, cert: cert }, options));
  server.on("stream", (stream: any) => {
    stream.respond({ ":status": 200 });
    stream.end("h2");
  });
  server.on("request", (req: any, res: any) => res.end("h1"));
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
  const port = server.address().port;
  const socket: any = tls.connect({
    port: port,
    host: "127.0.0.1",
    rejectUnauthorized: false,
    ALPNProtocols: offered,
    servername: "localhost",
  });
  const result = await withTimeout<string>(new Promise<string>((resolve) => {
    socket.on("secureConnect", () => resolve("alpnProtocol=" + JSON.stringify(socket.alpnProtocol)));
    socket.on("error", (e: any) => resolve("error=" + e.code));
  }), 800, "!! NEITHER secureConnect NOR error");
  console.log(label + ": offered=" + JSON.stringify(offered) + " -> " + result);
  socket.destroy();
  await sleep(50);
  await barrier(new Promise<void>((r) => server.close(() => r())), 300);
}

await alpnProbe("default secure server", {}, ["h2", "http/1.1"]);
await alpnProbe("default secure server", {}, ["http/1.1"]);
await alpnProbe("default secure server", {}, []);
await alpnProbe("allowHTTP1:true", { allowHTTP1: true }, ["h2", "http/1.1"]);
await alpnProbe("allowHTTP1:true", { allowHTTP1: true }, ["http/1.1"]);

// ---- end-to-end h2 over TLS ----
{
  const server: any = http2.createSecureServer({ key: key, cert: cert });
  server.on("stream", (stream: any, headers: any) => {
    stream.respond({ ":status": 200, "content-type": "text/plain", "x-scheme": headers[":scheme"] });
    stream.end("secure-ok");
  });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
  const port = server.address().port;
  const client: any = http2.connect("https://localhost:" + port, { ca: cert, rejectUnauthorized: false });
  const req: any = client.request({ ":path": "/s" });
  let status = 0;
  let scheme = "";
  const chunks: any[] = [];
  req.on("response", (h: any) => {
    status = h[":status"];
    scheme = h["x-scheme"];
  });
  req.on("data", (d: any) => {
    chunks.push(d);
  });
  req.resume();
  req.end();
  if (!(await waitEvent(req, "end", 1500))) console.log("!! h2-over-TLS stream never ended");
  const body = Buffer.concat(chunks).toString("utf8");
  console.log("h2 over TLS: status=" + status + " scheme=" + JSON.stringify(scheme) + " body=" + JSON.stringify(body));
  console.log("  session socket alpnProtocol:", JSON.stringify(client.socket.alpnProtocol));
  console.log("  session encrypted:", client.socket.encrypted === true);
  client.close();
  await sleep(80);
  await barrier(new Promise<void>((r) => server.close(() => r())), 300);
}

// ---- allowHTTP1 fallback carries a real HTTP/1.1 request ----
{
  const server: any = http2.createSecureServer({ key: key, cert: cert, allowHTTP1: true });
  server.on("request", (req: any, res: any) => {
    res.writeHead(200, { "content-type": "text/plain" });
    res.end("http" + req.httpVersion);
  });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", () => r()));
  const port = server.address().port;
  const body = await withTimeout<string>(new Promise<string>((resolve) => {
    const req = https.request(
      { host: "127.0.0.1", port: port, path: "/h1", rejectUnauthorized: false, ALPNProtocols: ["http/1.1"] },
      (res: any) => {
        let acc = "";
        res.setEncoding("utf8");
        res.on("data", (d: string) => {
          acc += d;
        });
        res.on("end", () => resolve("status=" + res.statusCode + " httpVersion=" + res.httpVersion + " body=" + acc));
      },
    );
    req.on("error", (e: any) => resolve("error=" + e.code));
    req.end();
  }), 1200, "!! NO RESPONSE");
  console.log("allowHTTP1 fallback request:", body);
  await barrier(new Promise<void>((r) => server.close(() => r())), 300);
}
