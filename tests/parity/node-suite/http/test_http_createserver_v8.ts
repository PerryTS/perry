import http from 'node:http';
import { setTimeout as wait } from 'node:timers/promises';

const server = http.createServer((req, res) => {
  res.writeHead(200, { 'Content-Type': 'text/plain' });
  res.end('hello\n');
});

const port = 18999;
await new Promise<void>(resolve => server.listen(port, () => resolve()));

const result = await fetch(`http://127.0.0.1:${port}/`);
const body = await result.text();
console.log(result.status, body.trim());  // 200 hello

server.close();
