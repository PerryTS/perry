import http from 'node:http';
import { Writable } from 'node:stream';
for (const mode of ['implicit', 'committed', 'explicit', 'empty']) {
  let raw = '';
  const socket = new Writable({ write(chunk, encoding, callback) { raw += chunk.toString(); callback(); } });
  const response = new http.ServerResponse({ method: 'GET', httpVersionMajor: 1, httpVersionMinor: 1 } as any);
  response.sendDate = false;
  response.assignSocket(socket as any);
  if (mode === 'explicit') response.setHeader('Content-Length', '5');
  if (mode !== 'implicit') response.writeHead(200);
  response.end(mode === 'empty' ? '' : 'hello');
  const boundary = raw.indexOf('\r\n\r\n');
  const headers = raw.slice(0, boundary).split('\r\n').filter(line => /^(content-length|transfer-encoding):/i.test(line));
  console.log(mode, JSON.stringify(headers), JSON.stringify(raw.slice(boundary + 4)));
}
