exports.a1 = 1;
const b = require("./cycle-b.cjs");
exports.fromB = b.seenA;
exports.a2 = 2;
