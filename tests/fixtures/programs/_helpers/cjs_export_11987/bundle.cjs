var ts = {}; ((module) => {
"use strict";
var __defProp = Object.defineProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var formats = require("./formats.cjs");
var bundle_exports = {};
__export(bundle_exports, {
  ANONYMOUS: () => ANONYMOUS,
  describe: () => describe,
});
module.exports = bundle_exports;
function describe(name) { return name ? name : ANONYMOUS; }
var ANONYMOUS = "anonymous function " + formats.RFC3986;
})(typeof module !== "undefined" && module.exports ? module : { exports: ts });
