// CJS-style entry that requires both cycle peers from the top-level. This
// mirrors readable-stream's `readable.js` shape which kicks off the
// _stream_readable / _stream_duplex cycle resolution. Both peers must load
// successfully and round-trip through new BHandle()/new AHandle().
'use strict';

var A = require('./peer_a.cjs');
var B = require('./peer_b.cjs');

var a = new A();
console.log('A.describe=' + a.describe());

var b = new B();
console.log('B.kind=' + b.kind);
console.log('B.tag=' + B.tag());

exports.ok = 'pass';
