// stream.promises.pipeline(streams..., async function) where the first stream has
// nothing buffered when the pipeline starts: the data is written afterwards (from
// a microtask, a timer, or a source piped in). The function stage must receive
// the live stream and see all of it, not a snapshot taken at the start.
import { Readable, PassThrough, Transform } from 'node:stream';
import streamP from 'node:stream/promises';

const timeout = (p, ms, name) =>
  Promise.race([p, new Promise((resolve) => setTimeout(() => resolve(name + ' TIMEOUT'), ms))]);
const identity = () => new Transform({ transform(chunk, enc, cb) { cb(null, chunk); } });
const total = async (stream) => {
  let n = 0;
  for await (const chunk of stream) n += chunk.length;
  return n;
};

const tests = {
  async piped() {
    const proxy = new PassThrough();
    Readable.from([Buffer.alloc(1000), Buffer.alloc(500)]).pipe(proxy);
    return streamP.pipeline(proxy, total);
  },
  async writtenFromTimer() {
    const proxy = new PassThrough();
    setTimeout(() => { proxy.write(Buffer.alloc(1000)); proxy.end(Buffer.alloc(500)); }, 5);
    return streamP.pipeline(proxy, identity(), total);
  },
  async writtenFromMicrotask() {
    const proxy = new PassThrough();
    Promise.resolve().then(() => { proxy.write(Buffer.alloc(1000)); proxy.end(Buffer.alloc(500)); });
    return streamP.pipeline(proxy, total);
  },
  async transformSource() {
    const first = identity();
    Promise.resolve().then(() => { first.write(Buffer.alloc(1000)); first.end(Buffer.alloc(500)); });
    return streamP.pipeline(first, identity(), total);
  },
};
for (const [name, run] of Object.entries(tests)) console.log(name, await timeout(run(), 3000, name));
process.exit(0);
