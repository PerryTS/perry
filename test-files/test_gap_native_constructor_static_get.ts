import { EventEmitter } from 'node:events';
import { Transform, Readable } from 'node:stream';
import { Buffer } from 'node:buffer';

class Child extends Transform {}
const C: any = Child;
const E: any = EventEmitter;
const T: any = Transform;
console.log('parent', Object.getPrototypeOf(C) === T);
for (const key of ['defaultMaxListeners', 'getMaxListeners', 'once', 'on']) {
  console.log('inherited', key, typeof C[key], C[key] === E[key]);
}
class R extends Readable {}
console.log('readable', typeof (R as any).from, (R as any).from === Readable.from);
class B extends Buffer {}
console.log('buffer', typeof (B as any).isBuffer, (B as any).isBuffer === Buffer.isBuffer);
const original = C.getMaxListeners;
C.getMaxListeners = 41;
console.log('shadow', C.getMaxListeners, typeof E.getMaxListeners);
delete C.getMaxListeners;
console.log('unshadow', C.getMaxListeners === original);
C.once = undefined;
console.log('undefined', typeof C.once, typeof E.once);
delete C.once;
console.log('undelete', C.once === E.once);
C.on = null;
console.log('null', C.on === null, typeof E.on);
delete C.on;
const parent = Object.getPrototypeOf(C);
Object.setPrototypeOf(C, { getMaxListeners: 73, arbitraryStatic: 91 });
console.log('reparent', C.getMaxListeners, C.arbitraryStatic, typeof C.once);
Object.setPrototypeOf(C, null);
console.log('null-parent', typeof C.getMaxListeners, typeof C.once);
Object.setPrototypeOf(C, parent);
console.log('restore-parent', C.getMaxListeners === E.getMaxListeners);
E.arbitraryStatic = 17;
console.log('new-native-static', C.arbitraryStatic);
E.arbitraryStatic = undefined;
console.log('native-undefined', typeof C.arbitraryStatic);
delete E.arbitraryStatic;
console.log('delete-native-static', typeof C.arbitraryStatic);
const once = E.once;
delete E.once;
console.log('delete-native-method', typeof C.once);
E.once = once;
console.log('restore-native-method', C.once === once);
let reads = 0;
let rightThis = false;
Object.defineProperty(E, 'customGetter', { configurable: true, get() { reads++; rightThis = this === C; return undefined; } });
console.log('getter', typeof C.customGetter, reads, rightThis);
delete E.customGetter;
