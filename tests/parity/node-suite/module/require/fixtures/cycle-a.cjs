exports.name = "a";
exports.before = true;
const b = require("./cycle-b.cjs");
exports.sawB = b.name;
exports.bSawAReady = b.sawAReady;
exports.ready = true;
