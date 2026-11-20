// Committing headers before end() prevents automatic Content-Length.
// Inspect raw bytes so an HTTP client cannot hide the framing difference.
import http from 'node:http';
import net from 'node:net';

function exchange(port: number, path: string, version: string, method: string, connection = 'close'): Promise<string> {
  return new Promise((resolve, reject) => {
    const socket = net.connect(port, '127.0.0.1');
    let raw = '';
    const timer = setTimeout(() => {
      socket.destroy();
      reject(new Error('response did not finish'));
    }, 5000);
    socket.on('connect', () => socket.write(
      `${method} /${path} HTTP/${version}\r\nHost: localhost\r\nConnection: ${connection}\r\n\r\n`));
    socket.on('data', chunk => { raw += chunk.toString(); });
    socket.on('end', () => { clearTimeout(timer); resolve(raw); });
    socket.on('error', error => { clearTimeout(timer); reject(error); });
  });
}

async function main() {
  const server = http.createServer((req, res) => {
    const path = req.url;
    res.sendDate = false;
    if (path === '/explicit') res.setHeader('Content-Length', '5');
    if (path === '/transfer') res.setHeader('Transfer-Encoding', 'chunked');
    if (path !== '/implicit') res.writeHead(path === '/204' ? 204 : path === '/304' ? 304 : 200);
    if (path === '/stream') res.write('he');
    if (path === '/empty' || path === '/204' || path === '/304') res.end();
    else res.end(path === '/stream' ? 'llo' : 'hello');
  });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  const port = typeof address === 'object' && address ? address.port : 0;
  try {
    for (const version of ['1.1', '1.0']) {
      for (const path of ['implicit', 'committed', 'explicit', 'empty', '204', '304']) {
        const raw = await exchange(port, path, version, 'GET');
        const boundary = raw.indexOf('\r\n\r\n');
        if (boundary < 0) throw new Error('missing response head');
        const headers = raw.slice(0, boundary).split('\r\n').filter(line =>
          /^(content-length|transfer-encoding):/i.test(line)).sort();
        console.log(version, path, JSON.stringify(headers), JSON.stringify(raw.slice(boundary + 4)));
      }
    }
    const closed = await exchange(port, 'committed', '1.0', 'GET', 'keep-alive');
    console.log('1.0 committed keep-alive closes', /connection: close/i.test(closed), !/keep-alive:/i.test(closed));
    for (const path of ['committed', 'explicit', 'transfer', 'stream']) {
      const method = path === 'committed' || path === 'explicit' ? 'HEAD' : 'GET';
      const raw = await exchange(port, path, '1.1', method);
      const boundary = raw.indexOf('\r\n\r\n');
      if (boundary < 0) throw new Error('missing response head');
      const headers = raw.slice(0, boundary).split('\r\n').filter(line =>
        /^(content-length|transfer-encoding):/i.test(line)).sort();
      console.log(method, path, JSON.stringify(headers), JSON.stringify(raw.slice(boundary + 4)));
    }
  } finally {
    server.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
