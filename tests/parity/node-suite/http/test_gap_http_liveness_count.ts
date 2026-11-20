import { createServer } from 'node:http';

let closed = 0;
for (let i = 0; i < 32; i++) {
  const server = createServer();
  server.unref();
  server.ref();
  server.on('close', () => { closed++; });
  server.close();
}
setImmediate(() => { console.log('closed', closed); });
