// Cycle peer B: top-level requires peer A and tries to use A.prototype
// IMMEDIATELY at module top level (`util.inherits(BHandle, A)`-style). This
// is the exact pattern that breaks readable-stream/lib/_stream_duplex.js
// without the cycle-break wrap.
'use strict';

var A = require('./peer_a.cjs');

// Mimic util.inherits: ctor.prototype = Object.create(parent.prototype).
// If A is a still-loading ESM namespace, `A.prototype` is undefined and
// this throws. The fix is the lazy/cycle classifier in wrap_commonjs which
// recognizes that peer_a's lazy require of peer_b would create the cycle
// and skips its static import.
function BHandle() { A.call(this); this.kind = 'B'; }
BHandle.prototype = Object.create(A.prototype, {
  constructor: { value: BHandle, enumerable: false, writable: true, configurable: true },
});

BHandle.tag = function () { return 'tagged-from-B'; };

module.exports = BHandle;
module.exports.tag = BHandle.tag;
