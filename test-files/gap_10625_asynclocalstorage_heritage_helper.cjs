'use strict';
// CommonJS half of test_gap_10625_asynclocalstorage_heritage.ts: the exact
// shape #10621 fixed for AsyncResource (undici's own heritage pattern),
// mirrored here for AsyncLocalStorage per #10625.
const { AsyncLocalStorage } = require('node:async_hooks');
const asyncHooks = require('node:async_hooks');

class ViaRequire extends AsyncLocalStorage {
  constructor() {
    super();
  }
}

// `require('node:async_hooks').AsyncLocalStorage` reached via a namespace
// member on a plain `require()` result (not destructured).
class ViaRequireNamespaceMember extends asyncHooks.AsyncLocalStorage {
  constructor() {
    super();
  }
}

module.exports = { ViaRequire, ViaRequireNamespaceMember };
