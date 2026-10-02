// #11744: earlier growth must preserve holes, aliases and named properties.
const left: any[] = [];
const right: any[] = [];
const alias = left;
console.log(left === right, left.length, right.length);
(left as any).label = 'kept';
for (let i = 0; i < 40; i++) left.push({ id: i });
console.log(alias === left, alias.length, (alias as any).label, alias[39].id);
left.length = 2;
left.length = 6;
console.log(Object.keys(left).join(','), 2 in left, left[4]);
left[5] = undefined;
console.log(4 in left, 5 in left);
class SmallArray extends Array {
  label = 'subclass';
}
const sub = new SmallArray();
const subAlias = sub;
for (let i = 0; i < 40; i++) sub.push(i);
console.log(sub === subAlias, sub instanceof SmallArray, sub.label, sub.length, sub[39]);
console.log(sub.shift(), sub.pop(), sub.length, sub[0]);
