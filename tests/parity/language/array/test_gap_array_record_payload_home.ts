// The cold indexed getter can move the array before the counted loop resumes.
declare function gc(): void;
function collect() {
  if (typeof gc === "function") gc();
  for (let i = 0; i < 80; i++) { const junk: any = {v: i}; }
}
function sum(input: any) {
  let total = 0;
  for (const value of input) {
    collect();
    total += value.v;
  }
  return total;
}
const input: any = [{v: 1}, {v: -1}, {v: 3}];
let reads = 0;
Object.defineProperty(input, "1", {
  configurable: true,
  get() { collect(); reads++; return {v: 2}; }
});
console.log(sum(input), reads);
console.log(sum(input), reads);
