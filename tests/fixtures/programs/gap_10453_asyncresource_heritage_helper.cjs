'use strict';
// CommonJS half of test_gap_10453_asyncresource_heritage.ts: the exact shape
// undici's API handlers use (`lib/api/api-request.js` etc.).
const { AsyncResource } = require('node:async_hooks');
const asyncHooks = require('node:async_hooks');

class Plain extends AsyncResource {
  constructor(type) {
    super(type);
  }
}

class InTry extends AsyncResource {
  constructor(type) {
    try {
      super(type);
    } catch (err) {
      throw err;
    }
  }
}

// `require('node:async_hooks').AsyncResource` reached via a namespace member
// on a plain `require()` result (not destructured).
class ViaMemberExport extends asyncHooks.AsyncResource {
  constructor(type) {
    super(type);
  }
}

module.exports = { Plain, InTry, ViaMemberExport };
