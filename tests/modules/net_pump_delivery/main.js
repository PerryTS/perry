import { createServer } from 'node:net';

// This checks event delivery only, not Error.code parity. Run in a scratch cwd:
// both a successful local bind and a sandbox-denied bind must settle callbacks.
const server = createServer({ allowHalfOpen: true }, socket => socket.end());
const timer = setTimeout(() => {
  console.error('FAIL: listen emitted neither listening nor error');
  process.exit(1);
}, 1500);
let errorEvents = 0;
server.on('error', () => { errorEvents++; });
const result = await new Promise((resolve, reject) => {
  function onError() { resolve('error'); }
  server.once('error', onError);
  server.listen(process.cwd() + '/pump-' + process.pid + '.sock', () => {
    server.removeListener('error', onError);
    server.close(error => error ? reject(error) : resolve('listening'));
  });
});
clearTimeout(timer);
if (result === 'error' && errorEvents !== 1) throw new Error('error listener not delivered exactly once');
console.log('PASS listen settled: ' + result);
