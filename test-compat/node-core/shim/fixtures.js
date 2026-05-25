'use strict';
//
// Perry-compilable replacement for Node's `test/common/fixtures.js` (#800).
// Resolves/reads files under the real Node `test/fixtures` tree; the runner
// exports its absolute location as `PERRY_NODE_CORE_FIXTURES`. CommonJS —
// see ./index.js.

const fs = require('fs');
const path = require('path');

const fixturesDir = process.env.PERRY_NODE_CORE_FIXTURES || '/nonexistent-fixtures';

// Rest params + iterative join instead of `Array.prototype.slice.call` +
// `.apply` (both trip Perry's #1777 gap).
function fixturesPath(...parts) {
  let p = fixturesDir;
  for (const part of parts) p = path.join(p, part);
  return p;
}

function readSync(...parts) {
  return fs.readFileSync(fixturesPath(...parts));
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
