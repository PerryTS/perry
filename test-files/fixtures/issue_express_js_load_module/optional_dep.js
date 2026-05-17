// V8-fallback CJS module that mirrors `debug/src/node.js`'s optional-dep
// pattern: a top-level `require()` for a peer dep that may not be installed,
// wrapped in a try/catch. Before the fix, the CJS-wrapped module hoisted
// `require('non-existent-optional-pkg-xyz')` to a static
// `import * as _req_0 from 'non-existent-optional-pkg-xyz'` at the top of
// the wrapper, and the unresolved bare specifier aborted module loading
// with `[js_load_module] FAILED to load`. After the fix, missing bare
// specifiers resolve to a `perry-missing:` stub and the require() call
// throws a MODULE_NOT_FOUND error from INSIDE the wrapper's `require()`
// function — caught by user `try/catch` exactly like Node.js.

var colorLevel = 0;

try {
  // eslint-disable-next-line global-require
  var supportsColor = require("non-existent-optional-pkg-xyz");
  if (supportsColor && supportsColor.level >= 2) {
    colorLevel = supportsColor.level;
  }
} catch (e) {
  // Swallow — optional dep, doesn't have to be installed.
  colorLevel = -1;
}

exports.colorLevel = colorLevel;
exports.label = "optional-dep-ok";
