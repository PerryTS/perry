// turnloop P5 — the `node:http` server's lifecycle edges, over a raw socket.
//
// The sibling `test_gap_turnloop_http_server.ts` pins the wire for ordinary
// exchanges; this one pins the parts that are easy to get wrong when the
// accept loop, the codec and the handler stop being three different tasks:
// `Expect: 100-continue` routed to `'checkContinue'`, trailers after a chunked
// body, a graceful `server.close()` with a request still in flight, and a
// client that disappears mid-request.
import http from 'node:http';
import net from 'node:net';

function scrub(raw: string): string {
  const out: string[] = [];
  let headers: string[] = [];
  const flush = () => {
    if (headers.length > 0) { headers.sort(); out.push(...headers); headers = []; }
  };
  for (const line of raw.split('\r\n')) {
    if (/^[A-Za-z][A-Za-z0-9-]*:\s/.test(line)) {
      if (!/^date:/i.test(line)) headers.push(line);
      continue;
    }
    flush();
    out.push(line);
  }
  flush();
  return out.join('\n');
}

// Resolve on a quiet period: several of these cases deliberately leave the
// connection open, so there is no EOF to wait for.
function exchange(port: number, request: string, quietMs = 250): Promise<string> {
  return new Promise((resolve) => {
    const sock = net.connect(port, '127.0.0.1');
    let buf = '';
    let quiet: ReturnType<typeof setTimeout> | null = null;
    const finish = () => {
      if (quiet) clearTimeout(quiet);
      try { sock.destroy(); } catch {}
      resolve(buf);
    };
    const bump = () => { if (quiet) clearTimeout(quiet); quiet = setTimeout(finish, quietMs); };
    sock.on('error', () => finish());
    sock.on('connect', () => { sock.write(request); bump(); });
    sock.on('data', (d) => { buf += d.toString('binary'); bump(); });
    sock.on('end', () => finish());
  });
}

async function main() {
  const events: string[] = [];

  const server = http.createServer((req, res) => {
    if (req.url === '/trailers') {
      res.setHeader('Content-Type', 'text/plain');
      res.setHeader('Trailer', 'X-Checksum');
      res.setHeader('Transfer-Encoding', 'chunked');
      res.write('part-one;');
      res.addTrailers({ 'X-Checksum': 'abc123' });
      res.end('part-two');
      return;
    }
    if (req.url === '/slow') {
      // Still in flight when `server.close()` runs below.
      setTimeout(() => {
        res.setHeader('Content-Type', 'text/plain');
        res.end('slow-done');
      }, 300);
      return;
    }
    if (req.url === '/reset') {
      req.on('aborted', () => events.push('request aborted'));
      setTimeout(() => {
        // The peer is gone by now; ending must not throw.
        try { res.end('too-late'); } catch (e: any) { events.push('end threw: ' + e.message); }
      }, 300);
      return;
    }
    res.setHeader('Content-Type', 'text/plain');
    res.end('ok');
  });

  server.on('checkContinue', (req: any, res: any) => {
    events.push('checkContinue ' + req.url);
    res.writeContinue();
    let size = 0;
    req.on('data', (c: Buffer) => { size += c.length; });
    req.on('end', () => {
      res.setHeader('Content-Type', 'text/plain');
      res.end('continued:' + size);
    });
  });

  await new Promise<void>((r) => server.listen(0, '127.0.0.1', () => r()));
  const address = server.address();
  const port = typeof address === 'object' && address ? address.port : 0;
  console.log('listening:', port > 0);

  // 1. Trailers after a chunked body.
  console.log('--- trailers ---');
  console.log(scrub(await exchange(port, 'GET /trailers HTTP/1.1\r\nHost: x\r\n\r\n')));

  // 2. `Expect: 100-continue` with a 'checkContinue' listener: the interim
  //    response has to reach the wire, since nothing sends it automatically
  //    once the listener has taken the request over.
  console.log('--- checkContinue ---');
  console.log(
    scrub(
      await exchange(
        port,
        'POST /expect HTTP/1.1\r\nHost: x\r\nExpect: 100-continue\r\nContent-Length: 4\r\n\r\nbody',
      ),
    ),
  );

  // 3. A client that vanishes mid-request. The server must survive it and keep
  //    serving; the late `res.end()` must not throw.
  await new Promise<void>((resolve) => {
    const sock = net.connect(port, '127.0.0.1');
    sock.on('error', () => {});
    sock.on('connect', () => {
      sock.write('GET /reset HTTP/1.1\r\nHost: x\r\n\r\n');
      setTimeout(() => { sock.destroy(); resolve(); }, 100);
    });
  });
  await new Promise<void>((r) => setTimeout(r, 500));
  console.log('--- after a client reset, the server still answers ---');
  console.log(scrub(await exchange(port, 'GET /after HTTP/1.1\r\nHost: x\r\n\r\n')));

  // 4. `server.close()` with a request in flight: Node stops accepting, lets
  //    the in-flight request finish, and only then fires 'close'.
  const slow = exchange(port, 'GET /slow HTTP/1.1\r\nHost: x\r\n\r\n', 600);
  await new Promise<void>((r) => setTimeout(r, 50));
  const closed = new Promise<void>((r) => server.close(() => { events.push("server 'close'"); r(); }));
  const slowBody = await slow;
  // Only the status line and the body: Perry answers `Connection: close` on a
  // response issued after `server.close()` while Node keeps `keep-alive`, and
  // that predates P5 (the hyper path computed the same override). What this
  // case is about is that the in-flight request finishes at all.
  console.log('--- in-flight request survived server.close() ---');
  console.log('status:', slowBody.split('\r\n')[0]);
  console.log('body:', slowBody.slice(slowBody.indexOf('\r\n\r\n') + 4));
  await closed;

  // 5. A connection refused after close proves the listener really stopped.
  const refused = await new Promise<string>((resolve) => {
    const sock = net.connect(port, '127.0.0.1');
    sock.on('error', (e: any) => resolve(e && e.code ? e.code : 'ERROR'));
    sock.on('connect', () => { sock.destroy(); resolve('CONNECTED'); });
  });
  console.log('after close, a new connection is:', refused);
  console.log('events:', events.join(' | '));
}

main();
