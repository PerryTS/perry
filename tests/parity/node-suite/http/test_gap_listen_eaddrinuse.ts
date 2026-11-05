// A listen() that fails to bind (EADDRINUSE) must emit 'error' on the server
// and must NOT emit 'listening' or run the listen(cb) callback — for every
// server flavour: http, https, http2 (h2c) and http2 secure. Node 26.5.1
// prints `error EADDRINUSE listen listening=false cb=false listeningEvent=false`
// for all four. Certificate material is the repository's inline CN=localhost
// fixture, as in test_gap_turnloop_https_server.ts.
import http from 'node:http';
import https from 'node:https';
import http2 from 'node:http2';

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
-----END PRIVATE KEY-----`;

function probe(label: string, server: any, port: number): Promise<void> {
  return new Promise((resolve) => {
    let cb = false;
    let listeningEvent = false;
    let err: any = null;
    server.on('listening', () => { listeningEvent = true; });
    server.on('error', (e: any) => { err = e; });
    server.listen(port, '127.0.0.1', () => { cb = true; });
    setTimeout(() => {
      const what = err ? `error ${err.code} ${err.syscall}` : 'no-error';
      console.log(`${label}: ${what} listening=${server.listening} cb=${cb} listeningEvent=${listeningEvent}`);
      if (server.listening) server.close();
      resolve();
    }, 300);
  });
}

const holder = http.createServer((_req: any, res: any) => res.end('x'));
holder.listen(0, '127.0.0.1', async () => {
  const port = holder.address().port;
  await probe('http', http.createServer(), port);
  await probe('https', https.createServer({ key: KEY, cert: CERT }), port);
  await probe('http2', http2.createServer(), port);
  await probe('http2-secure', http2.createSecureServer({ key: KEY, cert: CERT }), port);
  holder.close(() => console.log('done'));
});
