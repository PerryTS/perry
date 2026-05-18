// Test coverage for util.inherits + node:stream prototype scaffold
// (fix for express → send "Object prototype may only be an Object or
// null" crash). The actual `util.inherits` exists only in the V8
// fallback's `node:util` shim, and the bug surfaced via CJS
// `require('stream') / require('util')` in
// `node_modules/send/index.js:30,173`. So we route the assertion
// through a `.js` fixture (V8 fallback) and just import its summary.

import results from './fixtures/util_inherits_v8/inherits_mod.js';

console.log('typeof Stream:', results.typeofStream);
console.log('typeof Stream.prototype:', results.typeofStreamProto);
console.log('typeof Stream.Readable:', results.typeofStreamReadable);
console.log(
    'typeof Stream.Readable.prototype:',
    results.typeofStreamReadableProto,
);
console.log('SendStream.super_ === Stream:', results.sendStreamSuper);
console.log('Bar instanceof Foo:', results.barInstanceofFoo);
console.log('Bar.greet():', results.barGreet);
console.log('Bar.super_ === Foo:', results.barSuper);
