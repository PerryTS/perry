import { Readable, Writable, Duplex } from 'node:stream';
import { EventEmitter } from 'node:events';

const opts = {
  objectMode: true, highWaterMark: 7,
  read(this: any) { this.push(this.label); this.push(null); },
  write(this: any, chunk: any, _encoding: any, done: any) {
    console.log('write', this.label, chunk); done();
  },
};
const r1: any = new Readable(opts); r1.label = 'r1';
const r2: any = new Readable(opts); r2.label = 'r2';
console.log('read', r1.read(), r2.read(), r1.readableHighWaterMark);
const w1: any = new Writable(opts); w1.label = 'w1';
const w2: any = new Writable(opts); w2.label = 'w2';
w1.write('a'); w2.write('b');
console.log('views', r1._readableState.objectMode, w1._writableState.objectMode,
  r1._readableState.highWaterMark, w1._writableState.highWaterMark);
console.log('hidden', Object.keys(r1._readableState).some(k => k.startsWith('__perry')),
  Object.keys(w1._writableState).some(k => k.startsWith('__perry')));
const inherited: any = Object.create({ objectMode: true, highWaterMark: 9, read() {}, write(_c: any, _e: any, cb: any) { cb(); } });
const d = new Duplex(inherited);
console.log('duplex', d.readableHighWaterMark, d.writableHighWaterMark, d.allowHalfOpen);
const e: any = new EventEmitter();
const log: string[] = [];
function once(this: any, n: number) { log.push('once' + n + (this === e)); }
e.once('x', once);
const raw = e.rawListeners('x')[0];
console.log('wrapper', raw.listener === once, Object.keys(raw).join(','));
e.on('x', (n: number) => log.push('on' + n));
e.emit('x', 1); e.emit('x', 2); raw(3);
const sym = Symbol('event');
e.once(sym, () => log.push('symbol')); e.emit(sym); e.emit(sym);
e._events.direct = () => log.push('direct');
e.emit('direct');
console.log('events', log.join('|'), e.listenerCount('x'), e.rawListeners('x').length);
