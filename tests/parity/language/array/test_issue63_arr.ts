// Issue #63 benchmark: array literal allocation in hot loop
const N = 500000;

let sum = 0;

const s1 = Date.now();
for (let i = 0; i < N; i++) {
  const arr = [i, i + 1, i + 2];
  sum += arr[0] + arr[1] + arr[2];
}
console.log("arr_only:", Date.now() - s1);

const s2 = Date.now();
for (let i = 0; i < N; i++) {
  const s = "item_" + i;
  if (s.length === 0) sum += 1;
}
console.log("str_only:", Date.now() - s2);

const s3 = Date.now();
for (let i = 0; i < N; i++) {
  const obj: any = { x: i, y: i * 2 };
  sum += obj.x + obj.y;
}
console.log("obj_no_str:", Date.now() - s3);

const s4 = Date.now();
for (let i = 0; i < N; i++) {
  const arr = [i, i + 1, i + 2, i + 3];
  sum += arr[0] + arr[1] + arr[2] + arr[3];
}
console.log("arr4:", Date.now() - s4);

console.log("sum:", sum);
