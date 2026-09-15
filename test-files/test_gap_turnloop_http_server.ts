// turnloop P5 — the `node:http` server on turnloop handles.
//
// Everything here is asserted through a raw `net.Socket` client rather than
// `http.request`, because the point is the WIRE: the status line, the framing
// decision, the `Connection` / `Keep-Alive` pair and connection reuse are what
// changed transport, and an HTTP client would hide all four.
//
// Nothing host-specific is printed — no port, no Date header, no timing — so
// the output is byte-comparable against Node.
import http from 'node:http';
import net from 'node:net';

// Normalize a raw response stream for byte comparison:
//
// * drop `Date`, which is wall-clock;
// * sort each contiguous run of header lines. Header ORDER is a pre-existing
//   Perry-vs-Node difference (Perry appends `Content-Length` where it
//   synthesizes it, Node emits it last), and it is not what this test is
//   about — the status line, the header SET, the framing and the body are.
//   Sorting keeps those four assertable without folding an unrelated
//   divergence into this file.
function scrub(raw: string): string {
  const out: string[] = [];
  let headers: string[] = [];
  const flush = () => {
    if (headers.length > 0) {
      headers.sort();
      out.push(...headers);
      headers = [];
    }
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

// Resolve on a quiet period rather than on a parsed message boundary: the
// keep-alive cases deliberately leave the connection open, so there is no EOF
// to wait for, and a "saw the head" heuristic would race the body. What is
// printed is the accumulated bytes, so the wait affects only when the probe
// stops — never what it compares.
function exchange(port: number, request: string): Promise<string> {
  return new Promise((resolve) => {
    const sock = net.connect(port, '127.0.0.1');
    let buf = '';
    let quiet: ReturnType<typeof setTimeout> | null = null;
    const finish = () => {
      if (quiet) clearTimeout(quiet);
      try { sock.destroy(); } catch {}
      resolve(buf);
    };
    const bump = () => {
      if (quiet) clearTimeout(quiet);
      quiet = setTimeout(finish, 250);
    };
    sock.on('error', () => finish());
    sock.on('connect', () => { sock.write(request); bump(); });
    sock.on('data', (d) => { buf += d.toString('binary'); bump(); });
    sock.on('end', () => finish());
  });
}

async function main() {
  const server = http.createServer((req, res) => {
    const url = req.url ?? '/';
    if (url === '/plain') {
      // `setHeader` + `end(body)` rather than `writeHead`: Node only computes
      // a `Content-Length` while the header block is still open at `end()`
      // time, and falls back to chunked once `writeHead` has committed it.
      // Perry length-frames both shapes — a pre-existing difference (hyper
      // framed it the same way), and not what this file is about.
      res.setHeader('Content-Type', 'text/plain');
      res.end('hello');
      return;
    }
    if (url === '/echo') {
      const chunks: Buffer[] = [];
      req.on('data', (c: Buffer) => chunks.push(Buffer.from(c)));
      req.on('end', () => {
        const body = Buffer.concat(chunks).toString();
        res.setHeader('Content-Type', 'text/plain');
        res.end(`echo:${body}`);
      });
      return;
    }
    if (url === '/stream') {
      res.writeHead(200, { 'Content-Type': 'text/plain' });
      res.write('one-');
      res.write('two-');
      res.end('three');
      return;
    }
    if (url === '/empty') {
      res.statusCode = 204;
      res.end();
      return;
    }
    if (url === '/custom') {
      // `statusMessage` rather than `writeHead(status, reason, headers)`, for
      // the same framing reason as `/plain`: this case is about the reason
      // phrase reaching the wire, not about which framing `writeHead` picks.
      res.statusCode = 418;
      res.statusMessage = 'I Am A Teapot Really';
      res.setHeader('Content-Type', 'text/plain');
      res.end('tea');
      return;
    }
    if (url === '/close') {
      res.setHeader('Connection', 'close');
      res.setHeader('Content-Type', 'text/plain');
      res.end('bye');
      return;
    }
    res.statusCode = 404;
    res.setHeader('Content-Type', 'text/plain');
    res.end('nope');
  });

  await new Promise<void>((r) => server.listen(0, '127.0.0.1', () => r()));
  const address = server.address();
  const port = typeof address === 'object' && address ? address.port : 0;
  console.log('listening:', port > 0);

  // 1. A plain GET, with the whole head compared.
  const plain = await exchange(port, 'GET /plain HTTP/1.1\r\nHost: x\r\n\r\n');
  console.log('--- plain ---');
  console.log(scrub(plain));

  // 2. A POST whose body the handler reads through 'data'/'end'.
  const post = await exchange(
    port,
    'POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\nworld');
  console.log('--- post ---');
  console.log(scrub(post));

  // 3. A chunked upload: the same handler, framing decided by the decoder.
  const chunked = await exchange(
    port,
    'POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n3\r\ndef\r\n0\r\n\r\n');
  console.log('--- chunked upload ---');
  console.log(scrub(chunked));

  // 4. A streamed response: head flushed by the first write, chunked body.
  const streamed = await exchange(port, 'GET /stream HTTP/1.1\r\nHost: x\r\n\r\n');
  console.log('--- streamed ---');
  console.log(scrub(streamed));

  // 5. 204 has no body and no content-length.
  const empty = await exchange(port, 'GET /empty HTTP/1.1\r\nHost: x\r\n\r\n');
  console.log('--- 204 ---');
  console.log(scrub(empty));

  // 6. HEAD: the head of a GET, none of its bytes.
  const head = await exchange(port, 'HEAD /plain HTTP/1.1\r\nHost: x\r\n\r\n');
  console.log('--- HEAD ---');
  console.log(scrub(head));

  // 7. A custom reason phrase survives to the wire.
  const custom = await exchange(port, 'GET /custom HTTP/1.1\r\nHost: x\r\n\r\n');
  console.log('--- custom reason ---');
  console.log(scrub(custom));

  // 8. Keep-alive reuse: two requests, one connection, two responses.
  const pipelined = await exchange(
    port,
    'GET /plain HTTP/1.1\r\nHost: x\r\n\r\nGET /empty HTTP/1.1\r\nHost: x\r\n\r\n');
  console.log('--- two on one connection ---');
  console.log(scrub(pipelined));

  // 9. `Connection: close` from the handler ends the connection.
  const closed = await exchange(port, 'GET /close HTTP/1.1\r\nHost: x\r\n\r\n');
  console.log('--- connection close ---');
  console.log(scrub(closed));

  // 10. An HTTP/1.0 request without keep-alive: close-delimited, no reuse.
  const ten = await exchange(port, 'GET /plain HTTP/1.0\r\n\r\n');
  console.log('--- http/1.0 ---');
  console.log(scrub(ten));

  // 11. `keepAliveTimeout = 0` is Node's "never time out", NOT "no keep-alive":
  //     the response still says keep-alive and carries no Keep-Alive header.
  const zeroServer = http.createServer((_req, res) => res.end('z'));
  zeroServer.keepAliveTimeout = 0;
  await new Promise<void>((r) => zeroServer.listen(0, '127.0.0.1', () => r()));
  const zeroAddress = zeroServer.address();
  const zeroPort = typeof zeroAddress === 'object' && zeroAddress ? zeroAddress.port : 0;
  const zero = await exchange(zeroPort, 'GET / HTTP/1.1\r\nHost: x\r\n\r\n');
  console.log('--- keepAliveTimeout=0 ---');
  console.log(scrub(zero));
  await new Promise<void>((r) => zeroServer.close(() => r()));

  await new Promise<void>((r) => server.close(() => r()));
  console.log('closed');
}

main();
