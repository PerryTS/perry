// fetch(url, { dispatcher: new Agent({ connect: { ca } }) }) trusts `ca` for
// that request only, like Node. The fixtures are inline (a throwaway CA and a
// leaf for 127.0.0.1 it signed, valid for a century) so the test does not
// depend on the working directory. Run under Node, `undici` must be
// resolvable from the repository root.
import https from 'node:https';
import { Agent } from 'undici';

const CA = `-----BEGIN CERTIFICATE-----
MIIBnzCCAUWgAwIBAgIURdJT7SqC9dp5x6vYkVa++A01B04wCgYIKoZIzj0EAwIw
HDEaMBgGA1UEAwwRcGVycnktZ2FwLXRlc3QtY2EwIBcNMjYxMDMwMjM0ODE5WhgP
MjEyNjEwMDYyMzQ4MTlaMBwxGjAYBgNVBAMMEXBlcnJ5LWdhcC10ZXN0LWNhMFkw
EwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEEpdWMUoz4BEv9AUaAXz1TxDJ4b6BFTXJ
gAhM7ipRnkQcnQptbfOgBpv8A7kYOUp3igMnxnHaySM+7ylHMG7ynaNjMGEwHQYD
VR0OBBYEFHqcOdZ4QThawUjFTueFo9p+3DGWMB8GA1UdIwQYMBaAFHqcOdZ4QTha
wUjFTueFo9p+3DGWMA8GA1UdEwEB/wQFMAMBAf8wDgYDVR0PAQH/BAQDAgEGMAoG
CCqGSM49BAMCA0gAMEUCIQC7GWrJJTzg5rl4OYHQ8N1RDK1Zu0objQXMUUDKMjKM
UQIgVqf4Hum3YfGV6o7PtvEGgLNrlvrHJmqnAn3pYiZmBWY=
-----END CERTIFICATE-----`;
const CERT = `-----BEGIN CERTIFICATE-----
MIIBxzCCAW2gAwIBAgIUc6DeDVMqFr5UkZ99rNAeHwgrSJUwCgYIKoZIzj0EAwIw
HDEaMBgGA1UEAwwRcGVycnktZ2FwLXRlc3QtY2EwIBcNMjYxMDMwMjM0ODE5WhgP
MjEyNjEwMDYyMzQ4MTlaMBQxEjAQBgNVBAMMCTEyNy4wLjAuMTBZMBMGByqGSM49
AgEGCCqGSM49AwEHA0IABIxjTvPL/8p11/UdhB/yoFboJbBL3leH1r8sWwZOOQFK
1GzwS4dHe9FMh7KXw1MqPVsb6kTCxHhCOVjZMP/1zcqjgZIwgY8wGgYDVR0RBBMw
EYcEfwAAAYIJbG9jYWxob3N0MAwGA1UdEwEB/wQCMAAwDgYDVR0PAQH/BAQDAgeA
MBMGA1UdJQQMMAoGCCsGAQUFBwMBMB0GA1UdDgQWBBQsuaQyjiXqiB4qrPvTip9C
Us+CMDAfBgNVHSMEGDAWgBR6nDnWeEE4WsFIxU7nhaPaftwxljAKBggqhkjOPQQD
AgNIADBFAiBzpunk+V8ac/1ylKVapXSQnYVcJOk/MoQ74JuenzpxwAIhAIm2h5x0
ivhJ+2UvTmPqs3w12ucD9usfk2HLNFVUFQ0G
-----END CERTIFICATE-----`;
const KEY = `-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgTKWgce5Lfig5tCY1
o50G9/z25AyVqqlL0OGRC45NnVahRANCAASMY07zy//Kddf1HYQf8qBW6CWwS95X
h9a/LFsGTjkBStRs8EuHR3vRTIeyl8NTKj1bG+pEwsR4QjlY2TD/9c3K
-----END PRIVATE KEY-----`;

async function attempt(label: string, init?: any): Promise<void> {
  try {
    const response = await fetch(url, init);
    console.log(label, response.status, await response.text());
  } catch (e: any) {
    console.log(label, 'rejected:', e instanceof TypeError, e && e.message);
  }
}

let url = '';

async function main() {
  const server = https.createServer({ key: KEY, cert: CERT }, (req, res) => {
    res.setHeader('Content-Type', 'text/plain');
    res.end('trusted:' + req.url);
  });
  await new Promise<void>((r) => server.listen(0, '127.0.0.1', () => r()));
  const address = server.address();
  const port = typeof address === 'object' && address ? address.port : 0;
  url = `https://127.0.0.1:${port}/ping`;

  await attempt('default');
  await attempt('plain agent', { dispatcher: new Agent() });
  const trusting = new Agent({ connect: { ca: CA } });
  await attempt('ca agent', { dispatcher: trusting });
  await attempt('ca agent again', { dispatcher: trusting });
  // The connection that trusted the CA must not serve a request that did not ask for it.
  await attempt('default after ca agent');
  await attempt('unverified agent', {
    dispatcher: new Agent({ connect: { rejectUnauthorized: false } }),
  });

  console.log('done');
  process.exit(0);
}

main();
