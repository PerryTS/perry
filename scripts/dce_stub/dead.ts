// Bodies that can never run: with PERRY_FACTS_DCE=1 each is a trap and the output is unchanged.
function neverCalled() { return "never"; }
function alsoDead(x: number) { return neverCalled() + x; }
class Unused { m() { return 1; } get g() { return 2; } }
const deadArrow = () => alsoDead(1);
if (process.argv.length < 0) { console.log(deadArrow(), new Unused().m()); }
console.log("live", [1, 2, 3].map((x) => x * 2).join(","));
