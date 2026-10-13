import { Readable, Writable } from 'node:stream';

const readable = new Readable({ read() {}, highWaterMark: 17 });
const writable = new Writable({ write(_chunk, _encoding, callback) { callback(); }, highWaterMark: 19 });
const rs = (readable as any)._readableState;
const ws = (writable as any)._writableState;
console.log(Object.keys(rs).join(','));
console.log(Object.keys(ws).join(','));
rs.length = 3;
ws.length = 4;
ws.corked = 2;
console.log(readable.readableLength, writable.writableLength, writable.writableCorked);
console.log(rs.highWaterMark, ws.highWaterMark, typeof ws.onwrite);

const url: any = new URL('https://example.com:8443/a?x=1');
console.log(Object.keys(url).join(','), Object.getOwnPropertyNames(url).join(','));
url.extra = 7;
console.log(url.hostname, url.pathname, url.extra, Object.keys(url).join(','));
delete url.extra;
url.extra = 9;
url.searchParams.set('x', '2');
console.log(url.href, Object.keys(url).join(','));

const iterator = [11, 12][Symbol.iterator]();
const result: any = iterator.next();
console.log(Object.keys(result).join(','), result.value, result.done);
result.extra = 'x';
delete result.value;
result.value = 13;
console.log(Object.keys(result).join(','), result.value, result.extra);

const timer: any = setTimeout(() => {}, 1000);
timer.extra = 23;
console.log(timer.extra, typeof timer.ref, typeof timer.unref);
delete timer.extra;
timer.extra = 29;
console.log(timer.extra);
clearTimeout(timer);
readable.destroy();
writable.destroy();
