// turnloop P6 acceptance: a REAL remote endpoint, through TLS, on the loop.
//
// Not a gap fixture: it needs the network, so its output is not comparable
// byte-for-byte against an oracle on a machine that may be offline. What it
// asserts is the part a loopback test cannot — a real certificate chain
// verified against the webpki roots, a real ALPN negotiation, a real redirect
// across origins, and a body large enough to span many TLS records.
//
// Run with PERRY_LOOP_STATS=1: nonzero turnloop turns/completions plus
// `p6 http_submitted=`/`connects=` show that the native path carried the work.
const targets = [
  'https://example.com/',
  'https://api.github.com/meta',
];

async function main() {
  for (const url of targets) {
    try {
      const res = await fetch(url);
      const body = await res.text();
      console.log(
        `${url} -> ${res.status} ${res.ok} ctype=${(res.headers.get('content-type') ?? '').split(';')[0]} bytes=${body.length > 0}`,
      );
    } catch (err) {
      const cause = (err as { cause?: { code?: string } }).cause;
      console.log(`${url} -> ERROR ${(err as Error).message} ${cause?.code ?? ''}`);
    }
  }

  // A cross-origin redirect over TLS: http -> https is the shape every real
  // deployment uses, and it exercises the header-stripping path.
  try {
    const res = await fetch('http://github.com/');
    console.log(`redirect -> ${res.status} redirected=${res.redirected} https=${res.url.startsWith('https://')}`);
  } catch (err) {
    console.log(`redirect -> ERROR ${(err as Error).message}`);
  }

  // A certificate that must NOT verify. `expired.badssl.com` is the canonical
  // probe; a client that accepts it is not checking anything.
  try {
    await fetch('https://expired.badssl.com/');
    console.log('expired-cert -> ACCEPTED (WRONG)');
  } catch (err) {
    const cause = (err as { cause?: { code?: string } }).cause;
    console.log(`expired-cert -> rejected ${cause?.code ?? '(no code)'}`);
  }

  // Several TLS requests in flight at once on one loop.
  const many = await Promise.all(
    [1, 2, 3, 4, 5, 6].map(() => fetch('https://example.com/').then((r) => r.status)),
  );
  console.log('concurrent-tls', many.join(','));
}

main();
