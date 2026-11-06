globalThis.__issue8039AInits = (globalThis.__issue8039AInits || 0) + 1;
exports.phase = "a-partial";

const cycleBPath = process.cwd() + "/.next/server/cycle-b.js";
const cycleB = require(cycleBPath);

exports.seenByB = cycleB.sawA;
exports.phase = "a-final";
