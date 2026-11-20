import http2 from 'node:http2';
const CHUNK = 'x'.repeat(64 * 1024);
const N = 32;
const server: any = http2.createServer((_req: any, res: any) => {
  let i = 0; let drains = 0;
  const pump = () => {
    while (i < N) {
      i++;
      if (!res.write(CHUNK)) { res.once('drain', () => { drains++; pump(); }); return; }
    }
    res.end();
    console.log(`server: wrote all ${N}, drained=${drains > 0}`);
  };
  pump();
});
server.listen(0, '127.0.0.1', () => {
  const client = http2.connect(`http://127.0.0.1:${server.address().port}`);
  const req = client.request({ ':path': '/' });
  let total = 0;
  req.on('data', (c: any) => { total += c.length; });
  req.on('end', () => { console.log(`client: bytes=${total} expected=${N * CHUNK.length}`); client.close(); server.close(); });
  req.end();
  setTimeout(() => { console.log(`TIMEOUT client bytes=${total}`); process.exit(0); }, 5000).unref();
});
