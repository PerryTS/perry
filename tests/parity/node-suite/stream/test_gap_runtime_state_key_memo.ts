import { Writable, Readable } from 'node:stream';
import { EventEmitter } from 'node:events';

const proto = { objectMode: true, highWaterMark: 7 };
for (let i = 0; i < 6; i++) {
  const opts = Object.create(proto);
  if (i === 1) opts.highWaterMark = 9;
  if (i === 2) proto.highWaterMark = 11;
  if (i === 3) delete proto.highWaterMark;
  if (i === 4) Object.setPrototypeOf(opts, { objectMode: true, highWaterMark: 13 });
  const w = new Writable(opts);
  const r = new Readable(opts);
  console.log(i, w.writableObjectMode, w.writableHighWaterMark, r.readableObjectMode, r.readableHighWaterMark);
}

let observed = 0;
const accessorOpts = { highWaterMark: 17 };
Object.defineProperty(accessorOpts, 'objectMode', { get() { observed++; return true; } });
for (let i = 0; i < 2; i++) {
  const w = new Writable(accessorOpts);
  console.log('getter', w.writableHighWaterMark);
}
console.log('reads', observed);

let deepOpts: any = { objectMode: true, highWaterMark: 19 };
for (let i = 0; i < 270; i++) deepOpts = Object.create(deepOpts);
const deepWritable = new Writable(deepOpts);
const deepReadable = new Readable(deepOpts);
console.log('deep', deepWritable.writableHighWaterMark, deepReadable.readableHighWaterMark);

const emitter = new EventEmitter();
let total = 0;
function listener(value: number) { total += value; }
for (let i = 0; i < 4; i++) {
  emitter.once('value', listener);
  console.log('emit', emitter.emit('value', i + 1), emitter.emit('value', 100));
}
console.log('total', total, emitter.listenerCount('value'));
