"use strict";
// `y` is a declared key of the born exports layout (a store of it exists in
// the text), so its accessor define meets the hole and the object moves to
// its overflow table. Reads must still call the getter with `this` = exports.
if (Math.random() > 2) exports.y = 0;
let reads = 0;
Object.defineProperty(exports, "y", {
  enumerable: true,
  get: function () { reads++; return this === exports ? "this-ok" : "this-wrong"; },
});
let stored = [];
Object.defineProperty(exports, "z", {
  enumerable: true,
  get: function () { return stored.length; },
  set: function (v) { stored.push(this === exports ? v : "this-wrong"); },
});
if (Math.random() > 2) exports.z = 0;
exports.reads = function () { return reads; };
exports.stored = function () { return stored.join(","); };
