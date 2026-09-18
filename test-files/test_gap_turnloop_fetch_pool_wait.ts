// turnloop P6 regression: a request the connection pool PARKS must still be
// delivered when the connections ahead of it FAIL — and the process must still
// exit.
//
// The engine's pool admits at most `max_per_host = 16` connections per origin;
// the 17th concurrent request is parked. Admitting a parked request used to
// live only in `release()`, which is reached from exactly one place — a
// response that completed normally. Every failure path closes the connection
// directly, so sixteen concurrent requests that all failed left the
// seventeenth parked forever.
//
// THE ASSERTION IS THE EXIT, not the rejection. A parked request keeps
// `has_pending_requests()` true, which keeps Perry's event loop alive on a
// promise that can never settle — a hang, not a slowdown. A fixture that only
// checked "all 17 settled" would pass on the broken build if it called
// `process.exit()`, and would hang forever if it did not. So this file
// deliberately does NOT call `process.exit()`: reaching the last line and
// returning is the test. On a broken build the harness times out with no
// output after the counts line.
//
// The failure path used is a connect error to a port nothing listens on, which
// is the cheapest way to reach `fail_conn` -> `close_conn` without ever going
// through `release()`.
import http from 'node:http';

// A port the OS has just released: bind an ephemeral listener, read its port,
// close it. Nothing is listening there when the fetches below run.
function freePort(): Promise<number> {
  return new Promise((resolve) => {
    const probe = http.createServer();
    probe.listen(0, '127.0.0.1', () => {
      const address = probe.address();
      const port = typeof address === 'object' && address !== null ? address.port : 0;
      probe.close(() => resolve(port));
    });
  });
}

const CONCURRENCY = 24; // comfortably past the engine's 16-per-origin limit

async function main() {
  const dead = await freePort();

  const results = await Promise.allSettled(
    Array.from({ length: CONCURRENCY }, (_, i) => fetch(`http://127.0.0.1:${dead}/p${i}`)),
  );

  console.log('settled', results.length);
  console.log('all rejected', results.every((r) => r.status === 'rejected'));

  const names = new Set(
    results.map((r) => (r.status === 'rejected' ? (r.reason as Error).name : 'FULFILLED')),
  );
  console.log('reason names', [...names].sort().join(','));

  // Every one must carry a transport cause, not just a bare TypeError: a
  // request that was parked and then admitted has to go through the same
  // connect path as the rest.
  //
  // The PRESENCE of a code is asserted, not its value. Two dozen simultaneous
  // connects to a closed port do not all fail the same way — the kernel
  // answers some with ECONNREFUSED and some with ECONNRESET, and which is
  // which varies per run and per host. Asserting the exact set would be
  // asserting the kernel's scheduling, which is not what this file is about.
  const coded = results.every(
    (r) =>
      r.status === 'rejected' &&
      typeof (r.reason as { cause?: { code?: string } }).cause?.code === 'string' &&
      (r.reason as { cause: { code: string } }).cause.code.length > 0,
  );
  console.log('all carry a transport code', coded);

  // A second round on the SAME origin: the seats the first round took must all
  // have come back, or this one parks forever too.
  const second = await Promise.allSettled(
    Array.from({ length: CONCURRENCY }, (_, i) => fetch(`http://127.0.0.1:${dead}/q${i}`)),
  );
  console.log('second settled', second.length);
  console.log('second all rejected', second.every((r) => r.status === 'rejected'));

  // Deliberately no process.exit(): returning from here must let the loop
  // drain and the process exit on its own.
  console.log('done');
}

main();
