// Exercise callback-free array serialization, layout changes, and fallback
// after a primitive prefix. Retain emitted strings across moving collections.
const numbers: any[] = [];
for (let i = 0; i < 500; i++) numbers.push((i - 250) * 0.125);
const words: any[] = ["", "a", "abc", "longer string", "é", "東京", "🙂", "\ud800", "\udfff", "\n\"\\"];
const values: any[] = [[], [0, -0, NaN, Infinity, -Infinity, 5e-324, 1e-317],
  [true, false, null, undefined], numbers, words, [1, 2, { a: 3 }]];
const sparse: any[] = [1, 2, 3];
delete sparse[1];
values.push(sparse);
const extended: any[] = [1];
extended[40] = 2;
values.push(extended);

const retained: string[] = [];
let hash = 0;
let failures = 0;
for (let round = 0; round < 80; round++) {
  for (let i = 0; i < values.length; i++) {
    const text = JSON.stringify(values[i]);
    if (JSON.stringify(JSON.parse(text)) !== text) failures++;
    for (let j = 0; j < text.length; j++) hash = (hash * 33 + text.charCodeAt(j)) % 1000000007;
    retained.push(text);
  }
}
console.log("primitive arrays", retained.length, failures, hash, JSON.stringify(retained).length);

const changing: any[] = [1, 2, 3];
console.log(JSON.stringify(changing));
changing[1] = "two";
console.log(JSON.stringify(changing));
changing[1] = undefined;
console.log(JSON.stringify(changing));
changing[1] = { x: 2 };
console.log(JSON.stringify(changing));

const keys: string[] = [];
const custom: any = [1, 2, 3];
custom.toJSON = function(key: string) { keys.push("array:" + key); return ["custom", key]; };
const mixed: any[] = [1, 2, { toJSON(key: string) {
  keys.push("object:" + key);
  return { inner: JSON.stringify(words), array: custom };
} }];
console.log(JSON.stringify(mixed));
console.log(JSON.stringify({ value: custom }));
console.log(keys.join(","));

const cycle: any[] = [1, 2];
cycle.push(cycle);
try { JSON.stringify(cycle); }
catch (error: any) { console.log(error.name); }
console.log(JSON.stringify(["after", "cycle"]));
