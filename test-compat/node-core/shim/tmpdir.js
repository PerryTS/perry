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

function resolve() {
  const parts = Array.prototype.slice.call(arguments);
  return path.join.apply(path, [tmpPath].concat(parts));
}

module.exports = {
  path: tmpPath,
  refresh,
  resolve,
  hasEnoughSpace: function () { return true; },
};
