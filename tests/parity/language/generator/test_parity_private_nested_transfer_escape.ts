function* source(): Generator<number> { yield 7; yield 11; }
function* publish(input: Generator<number>): Generator<Generator<number>> { yield input; }
let total=0;
for (const iterator of publish(source())) {
  for (const value of iterator) total += value;
}
console.log("escaped child " + total);
