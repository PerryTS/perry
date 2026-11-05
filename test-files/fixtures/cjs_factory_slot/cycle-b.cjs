const a = require("./cycle-a.cjs");
exports.seenA = Object.keys(a).join(",") + "=" + a.a1;
