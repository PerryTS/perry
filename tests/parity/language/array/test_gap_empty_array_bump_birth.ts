// Empty births preserve identity, initialized slack, and aliases across growth.
declare function gc(): void;
function collect(): void { if (typeof gc === "function") gc(); }
function empty(): any[] { return []; }
const a = empty();
const b = empty();
console.log(a === b, a.length, b.length, 0 in a, Object.keys(a).length);
const alias = a;
for (let i = 0; i < 130; i++) a.push({ n: i, text: "entry-" + i });
collect();
console.log(alias === a, alias.length, alias[0].n, alias[129].text, b.length);
a.length = 0;
console.log(alias.length, 0 in alias, 129 in alias);
a.push("new");
console.log(alias[0], alias.length, 1 in alias);
let total = 0;
for (let i = 0; i < 20000; i++) {
  const values = empty();
  const read = () => values;
  values.push({ n: i }, "v-" + i);
  if (i % 5000 === 0) collect();
  const kept = read();
  if (kept === values) total += kept[0].n + kept.length;
}
console.log(total, alias[0], b.length);
