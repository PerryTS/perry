// A server whose certificate is self-signed with basicConstraints CA:TRUE is
// accepted when that exact certificate is supplied as a trusted CA, like Node
// (OpenSSL does not reject a CA certificate used as a server's own). The
// certificate still has to match the requested host and is trusted for that
// request only. The fixtures are inline and valid for a century so the test
// does not depend on the working directory. Run under Node, `undici` must be
// resolvable from the repository root.
import https from 'node:https';
import { Agent } from 'undici';

// SAN: IP:127.0.0.1
const GOOD_CERT = `-----BEGIN CERTIFICATE-----
MIIBwTCCAWigAwIBAgIUOeoespEaUQnK0thx1cyiTGYymuEwCgYIKoZIzj0EAwIw
JTEjMCEGA1UEAwwacGVycnktZ2FwLXNlbGYtc2lnbmVkLWdvb2QwIBcNMjYxMDMx
MDIxMTI0WhgPMjEyNjEwMDcwMjExMjRaMCUxIzAhBgNVBAMMGnBlcnJ5LWdhcC1z
ZWxmLXNpZ25lZC1nb29kMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE23vbEhdG
4yoex3zo5W6tUVMJhboCFlLpNWbK7YdTA1CNJUvu/3rmqFJFN5THOR+hb0NbQvtq
7eUyEQyy6bsi36N0MHIwHQYDVR0OBBYEFJwsiYTHG1xeyW8hKm5SL1O4iiUSMB8G
A1UdIwQYMBaAFJwsiYTHG1xeyW8hKm5SL1O4iiUSMA8GA1UdEwEB/wQFMAMBAf8w
DwYDVR0RBAgwBocEfwAAATAOBgNVHQ8BAf8EBAMCAoQwCgYIKoZIzj0EAwIDRwAw
RAIgJAlBPF3ZwD1I1Ifmiy8dkK8zALjS2eGb0SGV6ROH014CIEIIW+WjxUkm5lQR
4Em1FdZhoJYj89USDXxX2MJXHM5E
-----END CERTIFICATE-----`;
const GOOD_KEY = `-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgkLhnpbF8JNJkLvPM
mUcEd4ZwikS+RVuFsRrV7EeJLoihRANCAATbe9sSF0bjKh7HfOjlbq1RUwmFugIW
Uuk1Zsrth1MDUI0lS+7/euaoUkU3lMc5H6FvQ1tC+2rt5TIRDLLpuyLf
-----END PRIVATE KEY-----`;
// SAN: DNS:example.invalid
const OTHER_CERT = `-----BEGIN CERTIFICATE-----
MIIBzDCCAXGgAwIBAgIUfLWVpQ1aEAvcuubFzDrr0wPdMwAwCgYIKoZIzj0EAwIw
JDEiMCAGA1UEAwwZcGVycnktZ2FwLXNlbGYtc2lnbmVkLWJhZDAgFw0yNjEwMzEw
MjExMjRaGA8yMTI2MTAwNzAyMTEyNFowJDEiMCAGA1UEAwwZcGVycnktZ2FwLXNl
bGYtc2lnbmVkLWJhZDBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABBKCKtqiChiB
/dWCeiT31fyZYfhV4cx0C0UjVe8wt8+pOQreikwMmtRygnLFeILGmFqiFi+xKkaE
ng3nR/h1GY6jfzB9MB0GA1UdDgQWBBRjp0c55Zs5W3e/q4OEt3FQbooaKzAfBgNV
HSMEGDAWgBRjp0c55Zs5W3e/q4OEt3FQbooaKzAPBgNVHRMBAf8EBTADAQH/MBoG
A1UdEQQTMBGCD2V4YW1wbGUuaW52YWxpZDAOBgNVHQ8BAf8EBAMCAoQwCgYIKoZI
zj0EAwIDSQAwRgIhALLFBXpUSLcwg1VuLeZWkO4fnONi1MvGr7A29Zeu2JieAiEA
kNf6ZqL0Nnj0/lsfWpm7n0iAr9NWaQhrAq02F22DCCk=
-----END CERTIFICATE-----`;
const OTHER_KEY = `-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgxcG+xSHawuw3pjaK
uYn5jjlrktJY/rPSu7bLlgxezAuhRANCAAQSgiraogoYgf3Vgnok99X8mWH4VeHM
dAtFI1XvMLfPqTkK3opMDJrUcoJyxXiCxphaohYvsSpGhJ4N50f4dRmO
-----END PRIVATE KEY-----`;

async function attempt(label: string, url: string, init?: any): Promise<void> {
  try {
    const response = await fetch(url, init);
    console.log(label, response.status, await response.text());
  } catch (e: any) {
    console.log(label, 'rejected:', e instanceof TypeError);
  }
}

function serve(key: string, cert: string): Promise<{ server: any; url: string }> {
  const server = https.createServer({ key, cert }, (req, res) => {
    res.setHeader('Content-Type', 'text/plain');
    res.end('self-signed:' + req.url);
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      const port = typeof address === 'object' && address ? address.port : 0;
      resolve({ server, url: `https://127.0.0.1:${port}/ping` });
    });
  });
}

async function main() {
  const good = await serve(GOOD_KEY, GOOD_CERT);
  const other = await serve(OTHER_KEY, OTHER_CERT);

  await attempt('default', good.url);
  const trusting = new Agent({ connect: { ca: GOOD_CERT } });
  await attempt('ca agent', good.url, { dispatcher: trusting });
  await attempt('ca agent again', good.url, { dispatcher: trusting });
  // The connection that trusted the certificate must not serve a request that did not ask for it.
  await attempt('default after ca agent', good.url);
  // Trusting a CA certificate used as the server's own does not waive the host check.
  await attempt('wrong host', other.url, {
    dispatcher: new Agent({ connect: { ca: OTHER_CERT } }),
  });
  // Trusting a different certificate does not trust this one.
  await attempt('other ca', good.url, {
    dispatcher: new Agent({ connect: { ca: OTHER_CERT } }),
  });

  console.log('done');
  process.exit(0);
}

main();
