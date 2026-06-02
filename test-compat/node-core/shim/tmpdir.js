'use strict';
//
// Perry-compilable replacement for Node's `test/common/tmpdir.js` (#800).
// A single refreshable scratch dir under the OS temp dir; both runtimes share
// this shim so behaviour stays symmetric. CommonJS — see ./index.js.

const fs = require('fs');
const os = require('os');
const path = require('path');

const tmpPath = path.join(os.tmpdir(), 'perry-node-core-tmp');

function refresh() {
  try {
    fs.rmSync(tmpPath, { recursive: true, force: true });
  } catch (e) {
    // directory may not exist yet
  }
  fs.mkdirSync(tmpPath, { recursive: true });
}

function resolve(...parts) {
  // Avoid `Array.prototype.slice.call(arguments)` + `path.join.apply` — both
  // trip Perry's #1777 gap (builtin/stdlib methods aren't first-class values).
  let p = tmpPath;
  for (const part of parts) p = path.join(p, part);
  return p;
}

module.exports = {
  path: tmpPath,
  refresh,
  resolve,
  hasEnoughSpace: function () { return true; },
};
