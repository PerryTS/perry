// #1723 — the #503 dynamic-stdlib-dispatch guard must NOT refuse the auditable
// `ns[dynamicKey].staticMember` shape: the dynamic key selects a stdlib
// SUB-namespace (path.win32 / path.posix) and the member is a source-visible
// static name, so nothing is hidden (unlike the `ns[runtimeVar]()` obfuscation
// the guard targets). Before the fix this file was a hard `(#503)` compile
// error; the #800 node-core radar hit the same guard on Node's own
// test-path-glob.js (`path[platform].matchesGlob(...)`).
//
// This file is pure CommonJS (no top-level import/export), so #1711's cjs_wrap
// rewrites `require('path')` to a namespace binding — the exact path the radar
// exercises. It asserts the dynamic SUB-namespace PROPERTY reads that the fix
// unblocks (these match Node). Dynamic sub-namespace METHOD dispatch
// (`path[v].matchesGlob(...)` returning the right value at runtime) is a
// separate runtime-layer gap tracked in its own issue; this test stays on the
// property surface so it is byte-for-byte correct against Node today (#1740).
const path = require('path');

const expected: Record<string, { sep: string; delimiter: string }> = {
  win32: { sep: '\\', delimiter: ';' },
  posix: { sep: '/', delimiter: ':' },
};

let checked = 0;
for (const platform of ['win32', 'posix']) {
  // path[platform] is the dynamic stdlib sub-namespace selection; .sep /
  // .delimiter are the auditable static members.
  const sep = path[platform].sep;
  const delimiter = path[platform].delimiter;
  if (sep !== expected[platform].sep) {
    throw new Error(`${platform}.sep = ${JSON.stringify(sep)}`);
  }
  if (delimiter !== expected[platform].delimiter) {
    throw new Error(`${platform}.delimiter = ${JSON.stringify(delimiter)}`);
  }
  checked++;
}

console.log('ok', checked);
