const http = require('http');
const s = http.createServer(() => {});
console.log(typeof s.unref(), s.unref() === s);
console.log(typeof s.ref(), s.ref() === s);

// Chaining: createServer(cb).unref().listen(...) must work (the
// downstream break in test-http-request-method-delete-payload).
const server = http.createServer((req: any, res: any) => {
  res.end('ok');
}).unref();
server.listen(0, () => {
  console.log('listening on', typeof server.address().port);
  server.close();
});
