// Arrays retain their element iteration after named-property writes.
function show(label: string, input: any) {
  const visited: any[] = [];
  for (const value of input) visited.push(value);
  const [first, second, ...rest] = input;
  console.log(label, JSON.stringify(visited), JSON.stringify([...input]),
    JSON.stringify(Array.from(input)), JSON.stringify([first, second, rest]));
}
const named: any = [1, 2, 3];
named.pos = 1;
named.end = 2;
show("named", named);
console.log("named properties", named.pos, named.end,
  Object.keys(named).join("|"), JSON.stringify(named));

const own: any = [4, 5];
own.pos = 9;
own.end = 10;
own[Symbol.iterator] = function () {
  let n = 0;
  return { next() { return n < 2 ? { value: 70 + n++, done: false } : { done: true }; } };
};
show("own", own);

const holes: any = [1, , 3];
holes.pos = 1;
holes.end = 3;
show("holes", holes);
console.log("hole keys", Object.keys(holes).join("|"), JSON.stringify(holes));

class NamedArray extends Array<number> {}
const subclass: any = new NamedArray();
subclass.push(8, 9, 10);
subclass.pos = 2;
subclass.end = 4;
show("subclass", subclass);

const growing: any = [1, 2];
growing.pos = 0;
growing.end = 2;
const grown: number[] = [];
for (const value of growing) {
  grown.push(value);
  if (value === 1) growing.push(3);
}
console.log("grow", grown.join("|"));
const shrinking: any = [1, 2, 3];
shrinking.pos = 0;
shrinking.end = 3;
const shrunk: number[] = [];
for (const value of shrinking) {
  shrunk.push(value);
  shrinking.length = 1;
}
console.log("shrink", shrunk.join("|"));

const ordered: any = [10, 20];
ordered.end = 2;
ordered.pos = 1;
ordered.extra = 3;
delete ordered.pos;
ordered.pos = 4;
const dynamicKey = process.argv[2] || "end";
ordered[dynamicKey] = 5;
console.log("order", Object.keys(ordered).join("|"), JSON.stringify(ordered),
  ordered.pos, ordered.end);
function* delegated(input: any) { yield* input; }
console.log("yield", JSON.stringify([...delegated(named)]));
// Allocation-bearing backedges make the moving-GC witness exercise loop polls.
let checksum = 0;
for (let i = 0; i < 64; ++i) {
  const a: any = [{ value: i }, { value: i + 1 }];
  a.pos = i;
  a.end = i + 2;
  a.owner = { value: i };
  for (const item of a) checksum += item.value;
  checksum += a.owner.value;
}
console.log("gc", checksum);
