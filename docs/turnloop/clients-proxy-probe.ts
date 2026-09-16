// turnloop clients lane — the proxy CONNECT tunnel, as a PROBE rather than a
// gap fixture.
//
// It is not a gap fixture on purpose. Node's `fetch` (undici) does NOT read
// `HTTP_PROXY` / `HTTPS_PROXY` from the environment, so a fixture that set them
// would have Perry tunnel and Node go direct — a divergence that says nothing
// about Perry. The oracle for the routing decision is the sans-I/O test set in
// `crates/perry-stdlib/src/turnloop_client/tests.rs`, which asserts the CONNECT
// head, the absolute-form plaintext line, and the refusal of a non-2xx. THIS
// probe asserts the other half: that the transport actually runs, end to end,
// against a real proxy and a real TLS origin.
//
// Run it with the counters on, and check the number, not the exit code:
//
//   HTTP_PROXY=http://127.0.0.1:18923 \
//   HTTPS_PROXY=http://127.0.0.1:18922 \
//   NODE_TLS_REJECT_UNAUTHORIZED=0 PERRY_LOOP_STATS=1 ./probe
//
// A run whose `[perry-loop] p6 ... tunnels=` is 0 went direct and proves
// nothing, which is the failure mode this file exists to make visible. The
// ports are fixed (18920..18923, `PERRY_PROBE_PORT_BASE` moves them) because
// the environment has to be set before the process starts. Running it with no
// proxy variables is the control arm: same output, `tunnels=0`.
import https from 'node:https';
import http from 'node:http';
import net from 'node:net';

const CERT = `-----BEGIN CERTIFICATE-----
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
const KEY = `-----BEGIN PRIVATE KEY-----
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
-----END PRIVATE KEY-----`

let connects: string[] = [];
let absoluteForm: string[] = [];

// The TLS origin the tunnel has to reach.
const origin = https.createServer({ cert: CERT, key: KEY }, (req, res) => {
  res.writeHead(200, { 'content-type': 'text/plain' });
  res.end('origin-tls:' + req.url);
});

// A plaintext origin, for the no-tunnel proxy path.
const plainOrigin = http.createServer((req, res) => {
  res.writeHead(200, { 'content-type': 'text/plain' });
  res.end('origin-plain:' + req.url);
});

// A CONNECT proxy: read the request line, dial the target, answer 200, splice.
const tunnelProxy = net.createServer((client) => {
  client.once('data', (chunk: Buffer) => {
    const line = chunk.toString('utf8').split('\r\n')[0];
    const parts = line.split(' ');
    if (parts[0] !== 'CONNECT') {
      client.end('HTTP/1.1 405 Method Not Allowed\r\n\r\n');
      return;
    }
    connects.push(parts[1]);
    const hostPort = parts[1].split(':');
    const upstream = net.connect(Number(hostPort[1]), hostPort[0], () => {
      client.write('HTTP/1.1 200 Connection established\r\n\r\n');
      upstream.pipe(client);
      client.pipe(upstream);
    });
    upstream.on('error', () => client.destroy());
  });
  client.on('error', () => {});
});

// A forwarding proxy for plaintext targets: the request line is absolute-form.
const plainProxy = http.createServer((req, res) => {
  absoluteForm.push(req.url ?? '');
  const target = new URL(req.url ?? '');
  const upstream = http.request(
    { host: target.hostname, port: Number(target.port), path: target.pathname, method: req.method },
    (up) => {
      let body = '';
      up.on('data', (d: any) => { body += d.toString(); });
      up.on('end', () => {
        res.writeHead(up.statusCode ?? 200, { 'content-type': 'text/plain' });
        res.end(body);
      });
    }
  );
  upstream.on('error', () => { res.writeHead(502); res.end('bad gateway'); });
  upstream.end();
});

// FIXED ports, because the environment the engine reads is fixed before the
// process starts: a port chosen at listen time could not appear in
// `HTTPS_PROXY`. Override the base with `PERRY_PROBE_PORT_BASE` when another
// lane is already on these.
const BASE = Number(process.env.PERRY_PROBE_PORT_BASE ?? '18920');
const ORIGIN_TLS = BASE;
const ORIGIN_PLAIN = BASE + 1;
const PROXY_CONNECT = BASE + 2;
const PROXY_PLAIN = BASE + 3;

function listen(server: any, port: number, label: string): Promise<number> {
  return new Promise((resolve) => {
    server.listen(port, '127.0.0.1', () => {
      console.log('listening ' + label + ' ' + port);
      resolve(port);
    });
  });
}

async function main(): Promise<void> {
  const originPort = await listen(origin, ORIGIN_TLS, 'origin-tls');
  const plainPort = await listen(plainOrigin, ORIGIN_PLAIN, 'origin-plain');
  await listen(tunnelProxy, PROXY_CONNECT, 'proxy-connect');
  await listen(plainProxy, PROXY_PLAIN, 'proxy-plain');

  const httpsProxy = process.env.HTTPS_PROXY ?? '';
  const httpProxy = process.env.HTTP_PROXY ?? '';
  console.log('HTTPS_PROXY=' + httpsProxy);
  console.log('HTTP_PROXY=' + httpProxy);

  // Both requests go out regardless; whether they route through the proxies
  // depends on the environment the caller set. The counters are the proof.
  const secure = await fetch('https://localhost:' + originPort + '/secure');
  console.log('tls body: ' + (await secure.text()));

  const plain = await fetch('http://127.0.0.1:' + plainPort + '/plain');
  console.log('plain body: ' + (await plain.text()));

  console.log('proxy saw CONNECT: ' + JSON.stringify(connects));
  console.log('proxy saw absolute-form: ' + JSON.stringify(absoluteForm));

  origin.close();
  plainOrigin.close();
  tunnelProxy.close();
  plainProxy.close();
}

main();
