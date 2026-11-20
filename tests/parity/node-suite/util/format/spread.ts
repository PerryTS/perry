// #7720: `util.format(...args)` — the spread operand used to be folded in as a
// single array argument, so format inspected the array instead of consuming it
// as the format string plus its substitutions.
import util from "node:util";
import { format } from "node:util";

const args = ["%s is %d", "x", 7];

console.log("namespace:", util.format(...args));
console.log("named import:", format(...args));
console.log("mixed:", util.format("prefix %s", ...["y"]));
console.log("trailing extra:", util.format(...args, "tail"));
console.log("single:", util.format(...["plain"]));
console.log("no args:", JSON.stringify(util.format(...([] as string[]))));
console.log("objects:", util.format(...["%j", { a: 1 }]));
