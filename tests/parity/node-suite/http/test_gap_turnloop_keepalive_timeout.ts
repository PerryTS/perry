// turnloop P5 — `server.keepAliveTimeout`, including the value 0.
//
// P0 recorded that `keepAliveTimeout = 0` means "never time out" in Node but
// "no keep-alive" in Perry, which folded two decisions into one: Perry gated
// connection *reuse* on the timeout being non-zero, so a server that disabled
// the timeout answered `Connection: close` on every response.
//
// Measured on Node 26.5.1: a zero timeout answers `Connection: keep-alive`
// with no `Keep-Alive` header and never closes the idle connection, while a
// finite one advertises `timeout=floor(ms/1000)` and FINs at
// `keepAliveTimeout + keepAliveTimeoutBuffer` (300+0 at 305 ms, 300+1000 at
// 1301 ms, 1000+1000 at 2002 ms).
//
// The close time is bucketed rather than printed, so the output is stable
// while still failing if the deadline is never armed (`closed=false`) or fires
// at the wrong scale.
import http from 'node:http';
import net from 'node:net';

function probe(keepAliveTimeout: number, buffer: number, idleMs: number): Promise<string> {
  return new Promise((resolve) => {
    const server = http.createServer((_req, res) => res.end('z'));
    server.keepAliveTimeout = keepAliveTimeout;
    server.keepAliveTimeoutBuffer = buffer;
    server.listen(0, '127.0.0.1', () => {
      const a: any = server.address();
      const sock = net.connect(a.port, '127.0.0.1');
      let resp = '';
      let finAt: number | null = null;
      const t0 = Date.now();
      sock.on('data', (d) => { resp += d.toString(); });
      sock.on('end', () => { if (finAt === null) finAt = Date.now() - t0; });
      sock.on('error', () => {});
      sock.on('connect', () => sock.write('GET / HTTP/1.1\r\nHost: x\r\n\r\n'));
      setTimeout(() => {
        const ka = /^keep-alive:.*$/im.exec(resp);
        const conn = /^connection:.*$/im.exec(resp);
        const expected = keepAliveTimeout + buffer;
        const closed = finAt !== null;
        // Bucket rather than print the millisecond: within half the expected
        // window either side counts as "closed on schedule".
        const onSchedule = closed && finAt! >= expected * 0.5 && finAt! <= expected * 2;
        const out =
          `kat=${keepAliveTimeout} buf=${buffer} ` +
          `conn=${conn ? conn[0].trim().toLowerCase() : '(none)'} ` +
          `ka=${ka ? ka[0].trim().toLowerCase() : '(none)'} ` +
          `closed=${closed} onSchedule=${closed ? onSchedule : 'n/a'}`;
        try { sock.destroy(); } catch {}
        server.close(() => resolve(out));
      }, idleMs);
    });
  });
}

async function main() {
  console.log(await probe(300, 0, 1200));
  console.log(await probe(300, 1000, 2600));
  console.log(await probe(1000, 1000, 3200));
  console.log(await probe(0, 1000, 1600));
  console.log('done');
}
main();
