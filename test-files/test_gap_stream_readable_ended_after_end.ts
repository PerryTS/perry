// `readableEnded` is "the 'end' event was emitted", not "EOF was pushed":
// a PassThrough that was end()ed but not yet consumed is still not ended
// (code that pipes a request into a proxy checks this before reading).
import { createRequire } from "node:module";
const req = createRequire(import.meta.url);
const { PassThrough, Readable } = req('node:stream');

const p = new PassThrough();
p.write('abc');
p.end();
console.log('after end(): readableEnded', p.readableEnded);
p.on('data', () => {});
p.on('end', () => {
  console.log('on end: readableEnded', p.readableEnded);
});

const r = new Readable({ read() {} });
r.push('x');
r.push(null);
console.log('after push(null): readableEnded', r.readableEnded);
r.on('data', () => {});
r.on('end', () => {
  console.log('on end: readableEnded', r.readableEnded);
});
