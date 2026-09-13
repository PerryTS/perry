var _transform = require("./transform.cjs");
var readCount = 0;
Object.defineProperty(exports, "transform", {
  enumerable: true,
  get: function () {
    readCount++;
    return _transform.transform;
  }
});
Object.defineProperty(module.exports, "value", {
  enumerable: true,
  writable: true,
  value: function (value) { return value + 1; }
});
exports.reads = function () { return readCount; };
exports.update = function () {
  _transform.update();
  exports.value = function (value) { return value + 11; };
  exports.late = function (value) { return value + 11; };
};
