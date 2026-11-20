const N = 500000;
const start = Date.now();
let checksum = 0;
for (let i = 0; i < N; i++) {
  const obj: any = { x: i, y: i * 2, name: "item_" + i };
  checksum += obj.x;
}
const elapsed = Date.now() - start;
console.log("elapsed:", elapsed, "ms");
console.log("checksum:", checksum);
