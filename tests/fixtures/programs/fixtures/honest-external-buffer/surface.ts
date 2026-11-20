// Real napi_create_external_buffer producer. Prepare fixture-addon with
// scripts/run_honest_external_buffer.sh before running this differential file.
const addon = require('fixture-external-buffer');
const first = addon.create();
const second = addon.create();
const same = first;
console.log('identity', first !== second, same === first);
const map = new Map([[first, 1], [second, 2]]);
const set = new Set([first, second, same]);
const weak = new WeakMap([[first, 3], [second, 4]]);
console.log('keys', map.size, set.size, map.get(same), weak.get(first), weak.get(second));
console.log('brands', typeof first, first instanceof Buffer, first instanceof Uint8Array, Buffer.isBuffer(first), ArrayBuffer.isView(first));
console.log('own', Object.keys(first).join(','), Object.getOwnPropertyNames(first).join(','));
console.log('json', JSON.stringify(first));
console.log('string', String(first), Object.prototype.toString.call(first));
console.log('prototype', Object.getPrototypeOf(first) === Buffer.prototype);
let rejected = false;
try { Uint8Array.prototype.subarray.call({}, 0); }
catch (error) { rejected = error instanceof TypeError; }
console.log('brand check', rejected);
const view = first.subarray(1);
view[0] = 90;
console.log('bytes', first[0], first[1], first[2], String(first), String(second));
console.log('backing', first.buffer === first.buffer, view.buffer === first.buffer);
function indexed(value: Uint8Array): number {
    value[2] = 88;
    return value[0] + value[2];
}
console.log('typed access', indexed(second), String(second));
console.log('native lifetime', addon.released());
