'use strict'
// pg 8.22.0 lib/native/client.js:3-10 shape: an optional native addon,
// required unconditionally once this file's own init runs, wrapped in a
// try/catch that RE-THROWS rather than swallowing.
var Native
try {
  Native = require('./gap10437_missing_optional_dep.cjs')
} catch (e) {
  throw e
}
module.exports = Native
