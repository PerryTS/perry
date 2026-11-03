// Formatting without a reachable %j substitution needs no JSON engine.
console.log("literal %j");
console.log("%s %d", "ok", 7);
console.log("%%j", {a: 1});
console.log(1, 2);
const value = "last";
console.log(value);
console.log({a: 1}, 2);
console.log([1, 2], 3);
