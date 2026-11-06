// Force escape: store each array into an outer sink so scalar replacement can't fire.
const N = 500000;

const sink: (number[])[] = [];
for (let i = 0; i < 10; i++) sink.push([0, 0, 0]);

let sum = 0;

const s1 = Date.now();
for (let i = 0; i < N; i++) {
  const arr = [i, i + 1, i + 2];
  sink[i % 10] = arr;
  sum += arr[0] + arr[1] + arr[2];
}
console.log("triple_escape:", Date.now() - s1);

const s2 = Date.now();
for (let i = 0; i < N; i++) {
  const arr = [i, i + 1, i + 2, i + 3];
  sink[i % 10] = arr;
  sum += arr[0] + arr[1] + arr[2] + arr[3];
}
console.log("quad_escape:", Date.now() - s2);

// Mixed: 8-elem
const s3 = Date.now();
for (let i = 0; i < N; i++) {
  const arr = [i, i + 1, i + 2, i + 3, i + 4, i + 5, i + 6, i + 7];
  sink[i % 10] = arr;
  sum += arr[0] + arr[7];
}
console.log("eight_escape:", Date.now() - s3);

console.log("sum:", sum);
