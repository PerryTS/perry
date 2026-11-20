// Copying reads array elements and never copies the named layout.
const holes: any = [1, , 3];
holes.pos = 8;
holes.end = 10;
const spread = [...holes];
const from = Array.from(holes);
const mapped = holes.map((v: number) => v);
console.log("copies", JSON.stringify(spread), Object.keys(spread).join("|"),
  JSON.stringify(from), Object.keys(from).join("|"),
  JSON.stringify(mapped), Object.keys(mapped).join("|"));
console.log("names", spread.pos, from.pos, mapped.pos);

let reads = 0;
const source: any = [1, 2];
source.pos = 3;
Object.defineProperty(source, "0", {
  get() { reads++; return 9; }, configurable: true, enumerable: true
});
for (let operation = 0; operation < 4; operation++) {
  reads = 0;
  let result: any;
  if (operation === 0) result = [...source];
  else if (operation === 1) result = Array.from(source);
  else if (operation === 2) result = source.map((x: number) => x);
  else { const [a, b] = source; result = [a, b]; }
  console.log("indexed getter", operation, JSON.stringify(result), reads, result.pos);
}

let iteratorReads = 0;
Object.defineProperty(source, Symbol.iterator, {
  get() { iteratorReads++; return undefined; }, configurable: true
});
reads = 0;
console.log("arraylike", JSON.stringify(Array.from(source)), reads, iteratorReads);
try { [...source]; } catch (e) { console.log("spread undefined", e instanceof TypeError); }
delete source[Symbol.iterator];
console.log("iterator removed", Object.getOwnPropertySymbols(source).length,
  JSON.stringify([...source]));
let namedReads = 0;
Object.defineProperty(source, "end", {
  get() { namedReads++; return 5; }, configurable: true
});
console.log("quiet", Object.hasOwn(source, "end"), "end" in source,
  Object.keys(source).join("|"), namedReads);
delete source.end;
console.log("quiet delete", namedReads, Object.hasOwn(source, "end"));
