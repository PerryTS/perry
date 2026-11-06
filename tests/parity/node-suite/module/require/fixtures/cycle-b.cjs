exports.name = "b";
const a = require("./cycle-a.cjs");
exports.sawA = a.name;
exports.sawAReady = a.ready;
