// Exercise the proven typed representation before any prototype descriptor edit.
function sum(a: number[]): any {
  let total: any = 0;
  for (const x of a) total += x;
  return total;
}
const values = Array.prototype[Symbol.iterator];
const itp: any = Object.getPrototypeOf(values.call([]));
const next = itp.next;
const holes: number[] = [1,,3];
let text = "";
for (const value of holes) text += String(value) + ",";
console.log("proven holes", text);
const alias: number[] = [];
for (let i = 0; i < 100; i++) alias.push(i);
console.log("grown", sum(alias));
const mutable: number[] = [1,2];
let seen = "";
for (const value of mutable) {
  seen += value + ",";
  if (value === 1) {
    mutable.push(3);
    itp.next = () => ({done:true});
  }
}
itp.next = next;
console.log("captured", seen);
const overrides: number[] = [1,2];
(overrides as any)[Symbol.iterator] = function() {
  let i = 0;
  return {next: () => i++ === 0 ? {value:"a", done:false}
    : i === 2 ? {value:"b", done:false} : {done:true}};
};
console.log("typed arbitrary output", sum(overrides));
