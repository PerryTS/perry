Object.defineProperty(exports, "__esModule", { value: true });
var _transform = require("./transform.cjs");
Object.defineProperty(exports, "transform", {
  enumerable: true,
  get: function () {
    return _transform.transform;
  }
});
Object.defineProperty(module.exports, "value", {
  enumerable: true,
  writable: true,
  value: function (value) { return value + 1; }
});
exports.reads = function () { return _transform.reads(); };
exports.update = function () {
  _transform.update();
  exports.value = function (value) { return value + 11; };
  exports.late = function (value) { return value + 11; };
};
