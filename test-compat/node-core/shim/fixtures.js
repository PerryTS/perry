'use strict';
//
// Perry-compilable replacement for Node's `test/common/fixtures.js` (#800).
// Resolves/reads files under the real Node `test/fixtures` tree; the runner
// exports its absolute location as `PERRY_NODE_CORE_FIXTURES`. CommonJS —
// see ./index.js.

const fs = require('fs');
const path = require('path');

const fixturesDir = process.env.PERRY_NODE_CORE_FIXTURES || '/nonexistent-fixtures';

function fixturesPath() {
  const parts = Array.prototype.slice.call(arguments);
  return path.join.apply(path, [fixturesDir].concat(parts));
}

function readSync() {
  const args = Array.prototype.slice.call(arguments);
  return fs.readFileSync(fixturesPath.apply(null, args));
}

function readKey(name, enc) {
  return fs.readFileSync(path.join(fixturesDir, 'keys', name), enc);
}

module.exports = {
  fixturesDir,
  path: fixturesPath,
  fileURL: fixturesPath,
  readSync,
  readKey,
};
