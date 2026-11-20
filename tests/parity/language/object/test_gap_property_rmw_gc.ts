// Accessors and numeric coercion collect while the update result is live.
declare function gc(): void;
let checksum = 0;
function churn() {
  const live: any[] = [];
  for (let i = 0; i < 32; i++) live.push({ text: 'moving-' + i, data: [i, i + 1] });
  if (typeof gc === 'function') gc();
  checksum += live[31].data[1];
}
let value: any = 10n;
const target: any = {};
Object.defineProperty(target, 'x', {
  get() { churn(); return value; },
  set(v: any) { churn(); value = v; },
});
for (let i = 0; i < 20; i++) {
  console.log(String(target.x++), String(value));
  console.log(String(--target.x), String(value));
}
function key() { churn(); return 'x'; }
console.log(String(target[key()]++), String(value));
value = { valueOf() { churn(); return 30n; } };
console.log(String(target.x++), String(value));
console.log(checksum);
