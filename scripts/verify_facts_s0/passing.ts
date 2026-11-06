declare function gc(): void;
let getters = 0, keys = 0, conversions = 0;
class Cell { n = 7; get value() { getters++; return this.n; } }
function read(c: any) { return c.n; }
const cell = new Cell();
const key = { toString() { keys++; return "value"; } };
const convert = { valueOf() { conversions++; return 5; } };
const result = (cell as any)[key as any] + Number(convert);
gc();
const before = process.memoryUsage().heapUsed;
let sum = 0;
for (let i = 0; i < 1000; i++) sum += read(cell);
const bytes = process.memoryUsage().heapUsed - before;
console.log(result, sum, getters, keys, conversions, "complete");
if (process.argv[2] === "measure") console.log("allocated", bytes);
