const g = Object.getPrototypeOf(function* () {});
const a = Object.getPrototypeOf(async function () {});
const ag = Object.getPrototypeOf(async function* () {});
const code = process.argv[2] || "g.call = function () { return 915; }; a.call = function () { return 916; }; ag.call = function () { return 917; };";
new Function("g", "a", "ag", code)(g, a, ag);
const generator = (function* () { yield 1; }).call(null);
const asyncFunction = (async function () { return 1; }).call(null);
const asyncArrow = (async () => 1).call(null);
const asyncGenerator = (async function* () { yield 1; }).call(null);
const restore = process.argv[3] || "delete g.call; delete a.call; delete ag.call;";
new Function("g", "a", "ag", restore)(g, a, ag);
console.log(generator, asyncFunction, asyncArrow, asyncGenerator);
const bad = process.argv[4] || "g.call = undefined; a.call = 17;";
new Function("g", "a", bad)(g, a);
let genError = "no throw";
let asyncError = "no throw";
try { (function* () { yield 1; }).call(null); } catch (e) { genError = e.name; }
try { (async function () { return 1; }).call(null); } catch (e) { asyncError = e.name; }
new Function("g", "a", "ag", restore)(g, a, ag);
console.log(genError, asyncError);
