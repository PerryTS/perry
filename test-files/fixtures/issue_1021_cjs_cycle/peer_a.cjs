// Cycle peer A: defines `module.exports = AHandle` (a function with a
// .prototype just like Node's `function Readable() {...}` does) and lazily
// requires peer B from inside a function body — mirroring readable-stream's
// `_stream_readable.js` shape.
'use strict';

module.exports = AHandle;

function AHandle(options) {
  if (!(this instanceof AHandle)) return new AHandle(options);
  this.kind = 'A';
  // Lazy require — only fires when AHandle is actually constructed.
  var B = require('./peer_b.cjs');
  this.peer = B.tag();
}

AHandle.prototype.describe = function () {
  return 'A wrapping ' + (this.peer || '<unset>');
};
