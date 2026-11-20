// Character scanners must select representation from the current value.
import { Buffer } from 'node:buffer';
function read(s: any, p: any): any { return s.charCodeAt(p); }
function scan(s: any): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0;
  return h;
}
const positions: any[] = [undefined, NaN, -0, -0.5, -1, 1.8, Infinity, -Infinity, 2147483648, '1', null, false, true];
const inputs = ['', 'abc', 'abc'.repeat(32), 'éøĀ中', 'a😀z', '\ud800x\udfff', 'foo' + 'bar', Buffer.from('external input é').toString(), ['a', '中', '😀'].join('')];
for (const s of inputs) {
  console.log('scan', s.length, scan(s));
  for (const p of positions) console.log('read', read(s, p));
}
let state: any = 'abcdef';
let sum = 0;
for (let i = 0; i < 6; i++) {
  if (i === 1) state = 'éøĀ中😀';
  if (i === 3) state = Buffer.from('0123456789').toString();
  sum += state.charCodeAt(i);
}
console.log('change', sum);
let recvCalls = 0, indexCalls = 0;
function receiver(): any { recvCalls++; return 'abc'.repeat(20); }
const index: any = { valueOf() { indexCalls++; return 2; } };
console.log('coerce', receiver().charCodeAt(index), recvCalls, indexCalls);
const fake: any = { charCodeAt(p: any) { return 'fake:' + p; } };
console.log('override', read(fake, 2));
console.log('negative-fraction', read('abc', -0.75));
let extra = 0;
console.log('extra', read('abc', 1), ('abc' as any).charCodeAt(1, ++extra), extra);

const custom: any = { value: 41, charCodeAt(p: any) { return this.value + p; }, slice(p: any) { return this.value - p; } };
function customSlice(s: any, p: any): any { return s.slice(p); }
console.log('this', read(custom, 1), customSlice(custom, 2));
