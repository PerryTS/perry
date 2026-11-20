import net from 'node:net';
const srv = net.createServer((s) => {
  let got = 0;
  s.on('data', (d) => { got += d.length; });
  s.on('end', () => { console.log('server got', got); s.end(); srv.close(); });
});
srv.listen(0, '127.0.0.1', () => {
  const s = new net.Socket();
  s.on('error', (e: any) => console.log('error', e.code));
  s.on('close', () => { console.log('close'); srv.close(); });
  console.log('write', s.write('hi'));
  s.connect((srv.address() as any).port, '127.0.0.1', () => s.end());
});
