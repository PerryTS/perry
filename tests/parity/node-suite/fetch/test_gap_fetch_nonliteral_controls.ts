import { createServer } from 'node:http';
async function main() {
  let hits = 0;
  const server = createServer((req, res) => {
    hits++;
    if (req.url === '/redirect') {
      res.writeHead(302, { location: '/echo' }); res.end();
    } else {
      let body = '';
      req.on('data', chunk => { body += chunk.toString(); });
      req.on('end', () => res.end(req.method + ':' + body));
    }
  });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const url = 'http://127.0.0.1:' + (server.address() as any).port;
  for (const redirect of ['follow', 'manual', 'error']) {
    const opts: any = { redirect };
    try {
      const response = await fetch(url + '/redirect', opts);
      console.log(redirect, response.status, response.redirected, await response.text());
    } catch (error: any) { console.log(redirect, error.name); }
  }
  const controller = new AbortController();
  controller.abort();
  const opts = { signal: controller.signal };
  const before = hits;
  try { await fetch(url, opts); } catch (error) { console.log('aborted', (error as any).name); }
  const request = new Request(url, opts);
  try { await fetch(request); } catch (error) { console.log('request-aborted', (error as any).name); }
  console.log('abort-hits', hits - before);
  function stream(): ReadableStream<Uint8Array> {
    return new ReadableStream({
      start(controller) { controller.enqueue(new Uint8Array([65, 66])); controller.close(); },
    });
  }
  const streamed: any = { method: 'POST', body: stream(), duplex: 'half' };
  console.log('stream', await (await fetch(url, streamed)).text());
  const falsyKeepalive: any = { method: 'POST', body: stream(), duplex: 'half', keepalive: '' };
  console.log('stream-falsy-keepalive', await (await fetch(url, falsyKeepalive)).text());
  for (const bad of [
    { method: 'POST', body: stream() },
    { method: 'POST', body: stream(), duplex: 'half', keepalive: true },
    { method: 'BAD METHOD' }, { duplex: 'full' }, { redirect: 'invalid' }, { method: 'TRACE' },
    { method: 'GET', body: 'bad' }, { headers: null }, { signal: {} },
  ]) {
    let synchronous = false;
    try {
      const result = fetch(url, bad as any);
      synchronous = true;
      await result;
      console.log('invalid', 'accepted');
    } catch (error: any) { console.log('invalid', synchronous, error.name); }
    try { new Request(url, bad as any); console.log('request-invalid', 'accepted'); }
    catch (error: any) { console.log('request-invalid', error.name); }
  }
  const later = new AbortController();
  const pending = fetch(url, { signal: later.signal });
  later.abort();
  try { await pending; } catch (error) { console.log('later-abort', (error as any).name); }
  const throwing: any = { get headers() { throw 'getter'; } };
  try { await fetch(url, throwing); } catch (error) { console.log('getter-rejection', error); }
  try { new Request(url, throwing); } catch (error) { console.log('request-getter', error); }
  await new Promise<void>(resolve => server.close(() => resolve()));
}
main();
