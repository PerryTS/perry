'use strict';
// CommonJS half of test_gap_10448_stream_subclass_heritage.ts: the exact
// shape nodemailer uses everywhere (`const { Transform } =
// require('stream'); class X extends Transform`), plus the sibling
// Writable/Readable/Duplex destructured shapes the issue lists as broken
// the same way, and a namespace-member export for comparison.
//
// State is captured via public class fields, not a constructor-body
// assignment after `super(...args)` with a rest-param spread — that shape
// (`constructor(...args) { super(...args); ... }`) hits a separate,
// pre-existing gap (native stream methods go missing) independent of
// heritage shape or this issue; not exercised here to keep this test
// isolated to #10448's own defect.
const { Transform, Writable, Readable, Duplex } = require('stream');
const stream = require('stream');

class CjsTransform extends Transform {
  _transform(chunk, _enc, cb) {
    cb(null, String(chunk).toUpperCase());
  }
}

// `require('stream').Transform` reached via a namespace member on a plain
// `require()` result (not destructured) — control: this shape is already
// recognized statically (`is_genuine_node_stream_parent`).
class CjsViaMember extends stream.Transform {
  _transform(chunk, _enc, cb) {
    cb(null, String(chunk).toUpperCase());
  }
}

class CjsWritable extends Writable {
  captured = '';
  _write(chunk, _enc, cb) {
    this.captured += String(chunk).toUpperCase();
    cb();
  }
}

class CjsReadable extends Readable {
  _done = false;
  _read() {
    if (this._done) return;
    this._done = true;
    this.push('x');
    this.push('y');
    this.push(null);
  }
}

// Write-half only (no `_read`/push): proves the destructured `Duplex`
// heritage installs `_write` the same way `Writable` does, without
// depending on read/write event-ordering across engines.
class CjsDuplex extends Duplex {
  captured = '';
  _write(chunk, _enc, cb) {
    this.captured += String(chunk).toUpperCase();
    cb();
  }
}

module.exports = {
  CjsTransform,
  CjsViaMember,
  CjsWritable,
  CjsReadable,
  CjsDuplex,
};
