exports.transform = function (value) { return value + 1; };
exports.update = function () {
  exports.transform = function (value) { return value + 11; };
};
