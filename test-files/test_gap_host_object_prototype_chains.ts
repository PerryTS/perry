// The [[Prototype]] chain of native host objects: Object.getPrototypeOf must lead
// to Object.prototype and never return the receiver itself (a self link made a
// `while (p = Object.getPrototypeOf(p))` loop spin), and instanceof must follow
// the chain. Objects whose full chain matches node's are printed in full; for the
// rest (host classes that have no prototype chain yet, answering null) only the
// invariant is compared: the walk ends without ever meeting a self link.
// The modules are required by literal calls in a CommonJS helper, so the compiler
// links the native classes the walk below constructs.
import modules from "./gap_host_object_prototype_chains_helper.cjs";
const { fs, os, path, net, http, zlib, crypto, cp, events, stream } = modules;

const out = [];
const nameOf = (p) => {
  const d = Object.getOwnPropertyDescriptor(p, 'constructor');
  return d && typeof d.value === 'function' ? d.value.name : '?';
};
function chain(o) {
  const names = [];
  let p = o;
  for (let i = 0; i < 12; i++) {
    const next = Object.getPrototypeOf(p);
    if (next === null) return names.join(' > ') + ' > null';
    if (next === p) return names.join(' > ') + ' > SELF';
    names.push(nameOf(next));
    p = next;
  }
  return names.join(' > ') + ' > ...';
}
function survey(label, make, ...ctors) {
  try {
    const o = make();
    const tests = ctors.map((c) => {
      try { return (o instanceof c) ? 'y' : 'n'; } catch (e) { return 'throws'; }
    });
    out.push(label + ': ' + chain(o) + ' | instanceof ' + tests.join(','));
    return o;
  } catch (e) {
    out.push(label + ': ERROR ' + (e && e.message));
  }
}
function noSelfLink(label, make) {
  try {
    const o = make();
    let p = o;
    let state = 'ends';
    for (let i = 0; i < 64; i++) {
      const next = Object.getPrototypeOf(p);
      if (next === null) break;
      if (next === p) { state = 'SELF'; break; }
      p = next;
      if (i === 63) state = 'long';
    }
    out.push(label + ': walk ' + state);
    return o;
  } catch (e) {
    out.push(label + ': ERROR ' + (e && e.message));
  }
}
const E = events.EventEmitter, S = stream.Stream, R = stream.Readable, W = stream.Writable;
const D = stream.Duplex, T = stream.Transform;

const dir = process.env.WORK_DIR || fs.mkdtempSync(path.join(os.tmpdir(), 'hoc-'));
const file = path.join(dir, 'hoc.bin');
fs.writeFileSync(file, 'x');

// instanceof columns: Object, EventEmitter, Stream, Readable, Writable, then the specific constructor
const rs = survey('fs.ReadStream', () => fs.createReadStream(file), Object, E, S, R, W, fs.ReadStream);
rs.on('error', () => {});
const ws = survey('fs.WriteStream', () => fs.createWriteStream(path.join(dir, 'hoc-out.bin')), Object, E, S, R, W, fs.WriteStream);
ws.on('error', () => {});
survey('new fs.ReadStream', () => new fs.ReadStream(file), Object, E, S, R, W, fs.ReadStream)?.on('error', () => {});
noSelfLink('fs.Stats', () => fs.statSync(file));
survey('fs.Dirent', () => fs.readdirSync(dir, { withFileTypes: true })[0], Object, fs.Dirent);
survey('fs.Dir', () => { const d = fs.opendirSync(dir); d.closeSync(); return d; }, Object, fs.Dir);
survey('Readable', () => new R(), Object, E, S, R, W);
survey('Writable', () => new W(), Object, E, S, R, W);
survey('Duplex', () => new D(), Object, E, S, R, W, D);
survey('Transform', () => new T(), Object, E, S, R, W, D, T);
survey('PassThrough', () => new stream.PassThrough(), Object, E, S, R, W, D, T, stream.PassThrough);
survey('Readable.from', () => R.from(['a']), Object, E, S, R, W);
survey('EventEmitter', () => new E(), Object, E);
survey('net.Socket', () => new net.Socket(), Object, E, S, D, net.Socket);
survey('net.Server', () => net.createServer(), Object, E, net.Server);
noSelfLink('http.Server', () => http.createServer());
noSelfLink('http.Agent', () => new http.Agent());
noSelfLink('http.ClientRequest', () => { const r = http.request({ host: '127.0.0.1', port: 1, path: '/' }); r.on('error', () => {}); r.destroy(); return r; });
survey('zlib.Gzip', () => zlib.createGzip(), Object, E, S, R, W, D, T, zlib.Gzip);
survey('zlib.Deflate', () => zlib.createDeflate(), Object, E, S, R, W, D, T, zlib.Deflate);
survey('zlib.BrotliCompress', () => zlib.createBrotliCompress(), Object, E, S, T, zlib.BrotliCompress);
noSelfLink('crypto.Hash', () => crypto.createHash('sha256'));
noSelfLink('crypto.Hmac', () => crypto.createHmac('sha256', 'k'));
noSelfLink('crypto.Sign', () => crypto.createSign('sha256'));
const child = noSelfLink('ChildProcess', () => cp.spawn('cat', [], { stdio: ['pipe', 'pipe', 'pipe'] }));
if (child) {
  child.on('error', () => {});
  noSelfLink('ChildProcess.stdin', () => child.stdin);
  noSelfLink('ChildProcess.stdout', () => child.stdout);
  child.kill();
}
noSelfLink('process.stdout', () => process.stdout);
noSelfLink('process.stdin', () => process.stdin);
survey('timer', () => { const t = setTimeout(() => {}, 1); clearTimeout(t); return t; }, Object);
survey('AbortController', () => new AbortController(), Object);
survey('URL', () => new URL('http://a/'), Object, URL);
survey('Headers', () => new Headers(), Object, Headers);
survey('Blob', () => new Blob(['x']), Object, Blob);
survey('TextEncoder', () => new TextEncoder(), Object, TextEncoder);

// A user loop over the chain must end (the self link made it spin).
let hops = 0;
for (let p = rs; (p = Object.getPrototypeOf(p)) && hops < 1000;) hops++;
out.push('fs.ReadStream walk ends: ' + (hops < 1000) + ' hops ' + hops);
const rsProto = Object.getPrototypeOf(rs);
out.push('proto is fs.ReadStream.prototype: ' + (rsProto === fs.ReadStream.prototype));
out.push('ws proto is fs.WriteStream.prototype: ' + (Object.getPrototypeOf(ws) === fs.WriteStream.prototype));
out.push('proto chain reaches Readable.prototype: ' + R.prototype.isPrototypeOf(rs));
out.push('Stream.prototype.isPrototypeOf(rs): ' + S.prototype.isPrototypeOf(rs));
out.push('EventEmitter.prototype.isPrototypeOf(rs): ' + E.prototype.isPrototypeOf(rs));
out.push('Object.prototype.isPrototypeOf(rs): ' + Object.prototype.isPrototypeOf(rs));

rs.destroy();
ws.destroy();
// The servers and child above keep nothing alive after their handles close.
console.log(out.join('\n'));
fs.rmSync(dir, { recursive: true, force: true });
process.exit(0);
