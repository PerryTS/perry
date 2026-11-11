Object.defineProperty(exports, "__esModule", { value: true });
var transform = function (value) { return value + 1; };
var readCount = 0;
Object.defineProperty(exports, "transform", {
  enumerable: true,
  get: function () {
    readCount++;
    return transform;
  }
});
exports.reads = function () { return readCount; };
exports.update = function () {
  transform = function (value) { return value + 11; };
};
