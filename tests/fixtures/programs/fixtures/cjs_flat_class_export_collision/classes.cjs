const shared = 7;
class First { value() { return shared; } }
class Second { value() { return shared; } }
module.exports = First;
module.exports.Second = Second;
function helper() { return shared; }
module.exports.shared = 99;
module.exports.helper = helper;
module.exports.First = First;
