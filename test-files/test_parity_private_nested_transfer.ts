function* source(start: number, end: number): Generator<number> {
  for (let i = start; i < end; i++) yield i;
}
function* mapOwned(input: Generator<number>, transform: (v: number) => number): Generator<number> {
  for (const value of input) yield transform(value);
}
let total = 0;
for (const value of mapOwned(source(0, 100), function (value: number): number { return value % 13; })) total += value;
console.log("nested " + total);
