// turnloop P6 — outbound `fetch` on turnloop handles.
//
// Everything is asserted against a local `node:http` server so the comparison
// against Node is deterministic: no remote endpoint, no wall clock, no port and
// no host-specific string is printed.
//
// Request headers are printed through an ALLOWLIST rather than in full. Two of
// them differ between the engines for reasons that predate this phase and are
// not what this file is about: Node's fetch sends `accept-encoding: gzip,
// deflate, br, zstd` (undici decompresses transparently) where Perry's reqwest
// build enables no decompression feature and sends none, and the two send
// different `user-agent` strings. Printing them would fold those into this
// test.
import http from 'node:http';
import zlib from 'node:zlib';

const ALLOWED = ['host-shape', 'content-type', 'content-length', 'x-probe', 'authorization'];

function pickHeaders(headers: Record<string, string | string[] | undefined>): string {
  const out: string[] = [];
  for (const name of ALLOWED) {
    const value = name === 'host-shape'
      ? (typeof headers['host'] === 'string' ? 'present' : 'absent')
      : headers[name];
    if (value !== undefined) out.push(`${name}=${Array.isArray(value) ? value.join(',') : value}`);
  }
  return out.join(' ');
}

let connections = 0;

const gzipped = zlib.gzipSync(Buffer.from('compressed-payload-0123456789'));

const server = http.createServer((req, res) => {
  const chunks: Buffer[] = [];
  req.on('data', (c: Buffer) => chunks.push(Buffer.from(c)));
  req.on('end', () => {
    const body = Buffer.concat(chunks).toString('utf8');
    const url = req.url ?? '/';
    if (url === '/plain') {
      res.setHeader('content-type', 'text/plain');
      res.setHeader('x-echo-method', req.method ?? '');
      res.setHeader('x-echo-headers', pickHeaders(req.headers));
      res.end('hello-from-server');
    } else if (url === '/echo') {
      res.setHeader('content-type', 'application/json');
      res.end(JSON.stringify({ method: req.method, body, headers: pickHeaders(req.headers) }));
    } else if (url === '/missing') {
      res.statusCode = 404;
      res.setHeader('content-type', 'text/plain');
      res.end('nope');
    } else if (url === '/hop') {
      res.statusCode = 302;
      res.setHeader('location', '/landed');
      res.end('moved');
    } else if (url === '/landed') {
      res.setHeader('content-type', 'text/plain');
      res.end('after-redirect');
    } else if (url === '/gzip') {
      res.setHeader('content-type', 'text/plain');
      res.setHeader('content-encoding', 'gzip');
      res.end(gzipped);
    } else if (url === '/binary') {
      res.setHeader('content-type', 'application/octet-stream');
      res.end(Buffer.from([0, 1, 2, 250, 251, 252, 253, 254, 255]));
    } else if (url.startsWith('/slot')) {
      res.setHeader('content-type', 'text/plain');
      res.end(`slot${url.slice('/slot'.length)}`);
    } else {
      res.statusCode = 400;
      res.end('bad');
    }
  });
});

server.on('connection', () => { connections += 1; });

// A port the OS has just released: bind an ephemeral listener, read its port
// and close it. Nothing is listening there when the fetch below runs.
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

function listen(): Promise<number> {
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      resolve(typeof address === 'object' && address !== null ? address.port : 0);
    });
  });
}

async function main() {
  const port = await listen();
  const base = `http://127.0.0.1:${port}`;

  // 1. a plain GET: status, statusText, ok, headers and body.
  const plain = await fetch(`${base}/plain`);
  console.log('plain status', plain.status, plain.statusText, plain.ok);
  console.log('plain type', plain.headers.get('content-type'));
  console.log('plain method-seen', plain.headers.get('x-echo-method'));
  console.log('plain headers-seen', plain.headers.get('x-echo-headers'));
  console.log('plain body', await plain.text());

  // 2. a POST with a JSON body and a custom header.
  const posted = await fetch(`${base}/echo`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', 'x-probe': 'p6' },
    body: JSON.stringify({ n: 7 }),
  });
  const echoed = await posted.json();
  console.log('post method', echoed.method);
  console.log('post body', echoed.body);
  console.log('post headers', echoed.headers);

  // 3. a POST with NO body still frames itself as content-length: 0.
  const empty = await fetch(`${base}/echo`, { method: 'POST' });
  const emptyEchoed = await empty.json();
  console.log('empty-post headers', emptyEchoed.headers);

  // 4. a non-2xx response is NOT an error; `ok` is false and the body is read.
  const missing = await fetch(`${base}/missing`);
  console.log('404 status', missing.status, missing.statusText, missing.ok);
  console.log('404 body', await missing.text());

  // 5. a redirect is followed, and both `url` and `redirected` report it.
  const hopped = await fetch(`${base}/hop`);
  console.log('redirect status', hopped.status, hopped.ok);
  console.log('redirect path', new URL(hopped.url).pathname);
  console.log('redirect flag', hopped.redirected);
  console.log('redirect body', await hopped.text());

  // 6. a `Content-Encoding: gzip` response is decoded before it reaches JS.
  const compressed = await fetch(`${base}/gzip`);
  console.log('gzip status', compressed.status);
  console.log('gzip body', await compressed.text());

  // 7. a binary body survives byte for byte through arrayBuffer().
  const binary = await fetch(`${base}/binary`);
  const bytes = new Uint8Array(await binary.arrayBuffer());
  console.log('binary bytes', Array.from(bytes).join(','));

  // 8. several requests in flight at once on one loop.
  const slots = await Promise.all(
    [1, 2, 3, 4, 5].map((n) => fetch(`${base}/slot${n}`).then((r) => r.text())),
  );
  console.log('concurrent', slots.join('|'));

  // 9. an aborted fetch rejects with AbortError and nothing else changes.
  const controller = new AbortController();
  const pending = fetch(`${base}/plain`, { signal: controller.signal });
  controller.abort();
  try {
    await pending;
    console.log('abort', 'NOT-REJECTED');
  } catch (err) {
    console.log('abort', (err as Error).name);
  }

  // 10. an already-aborted signal rejects before anything is dispatched.
  const pre = new AbortController();
  pre.abort();
  try {
    await fetch(`${base}/plain`, { signal: pre.signal });
    console.log('pre-abort', 'NOT-REJECTED');
  } catch (err) {
    console.log('pre-abort', (err as Error).name);
  }

  // 11. a connection that cannot be made reports a TypeError with a cause
  // carrying Node's code + syscall. The port is one the OS just handed back and
  // nothing is listening on, rather than a literal: WHATWG fetch blocks a list
  // of well-known ports outright (port 1 rejects with "bad port" on Node before
  // any socket is created), which would test the blocklist instead of the
  // transport.
  const deadPort = await freePort();
  try {
    await fetch(`http://127.0.0.1:${deadPort}/never`);
    console.log('refused', 'NOT-REJECTED');
  } catch (err) {
    const cause = (err as { cause?: { code?: string; syscall?: string } }).cause;
    console.log('refused', (err as Error).name, (err as Error).message, cause?.code, cause?.syscall);
  }

  // 12. an unresolvable name reports getaddrinfo ENOTFOUND, with Node's errno.
  try {
    await fetch('http://p6-does-not-exist.invalid/x');
    console.log('dns', 'NOT-REJECTED');
  } catch (err) {
    const cause = (err as { cause?: { code?: string; errno?: number; syscall?: string } }).cause;
    console.log('dns', (err as Error).name, cause?.code, cause?.errno, cause?.syscall);
    console.log('dns message', cause?.message);
  }

  // 13. the server saw FEWER connections than requests: the pool reused them.
  // Seventeen requests were issued above; a client with no keep-alive opens one
  // socket each. The exact count differs between engines (Node's undici pools
  // per-origin with its own limits), so what is asserted is the PROPERTY.
  console.log('reused-connections', connections < 12);

  server.close();
}

main();
