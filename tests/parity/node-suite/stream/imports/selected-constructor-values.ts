import { Readable, Writable } from 'node:stream';
import { inherits } from 'node:util';
class Body extends Readable { _read() {} }
const body = new Body();
console.log('heritage', body instanceof Readable, typeof body._readableState);
function Legacy(this: any) { Readable.call(this, { read() {} }); }
inherits(Legacy, Readable);
const legacy: any = new (Legacy as any)();
legacy.push('ok');
console.log('call', legacy instanceof Readable, String(legacy.read()), typeof legacy._readableState);
function Sink(this: any) { Writable.apply(this, [{ write(_chunk: any, _enc: any, cb: any) { cb(); } }]); }
inherits(Sink, Writable);
const sink: any = new (Sink as any)();
console.log('apply', sink instanceof Writable, typeof sink._writableState, sink.write('ok'));
const applyWritable = Function.prototype.apply.bind(Writable);
function Adapted(this: any) { applyWritable(this, [{ write(_c: any, _e: any, cb: any) { cb(); } }]); }
inherits(Adapted, Writable);
const adapted: any = new (Adapted as any)();
console.log('adapter', adapted instanceof Writable, adapted.write('ok'));
