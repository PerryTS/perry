// CommonJS name resolution: require('node:dns'), require('dns'), dns.promises and
// net.connect({ host: 'localhost' }) to a local listener. Hermetic: only
// `localhost` and numeric addresses are resolved (no resolver query leaves the
// machine), and the listener takes an ephemeral port.
// The CommonJS helper holds the literal `require` calls: the dns export router
// is armed by a literal `require('node:dns')` in some module of the program.
import helper from "./gap_dns_cjs_namespace_helper.cjs";
const { dns, bare, dnsPromises, net } = helper;

const isLoopback = (a) => a === '127.0.0.1' || a === '::1';
const step = (name, fn) => new Promise((resolve) => fn((...r) => { console.log(name, ...r); resolve(); }));

async function main() {
  console.log('exports', typeof dns.lookup, typeof dns.promises.lookup, typeof dnsPromises.lookup, bare.lookup === dns.lookup);
  await step('lookup', (done) => dns.lookup('localhost', (err, address, family) =>
    done(err === null, isLoopback(address), family === 4 || family === 6)));
  await step('lookup all', (done) => bare.lookup('localhost', { all: true }, (err, all) =>
    done(err === null, all.length > 0, all.every((a) => isLoopback(a.address)))));
  await step('lookup literal', (done) => dns.lookup('127.0.0.1', (err, address, family) =>
    done(err === null, address, family)));
  const one = await dns.promises.lookup('localhost');
  console.log('promises', isLoopback(one.address), one.family === 4 || one.family === 6);
  const all = await dnsPromises.lookup('localhost', { all: true });
  console.log('promises all', all.length > 0);
  const literal = await dns.promises.lookup('127.0.0.1');
  console.log('promises literal', literal.address, literal.family);

  const server = net.createServer((socket) => socket.end('hello by name'));
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address();
  const body = await new Promise((resolve, reject) => {
    let received = '';
    const socket = net.connect({ host: 'localhost', port });
    socket.setEncoding('utf8');
    socket.on('data', (d) => (received += d));
    socket.on('end', () => resolve(received));
    socket.on('error', reject);
  });
  console.log('connect', JSON.stringify(body));
  server.close();
}
main().catch((e) => { console.log('failed', e && e.stack || e); process.exitCode = 1; });
