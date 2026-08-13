globalThis.__issue8039BInits = (globalThis.__issue8039BInits || 0) + 1;

const cycleAPath = process.cwd() + "/.next/server/cycle-a.js";
const cycleA = require(cycleAPath);

exports.sawA = cycleA.phase;
