declare function gc(): void;
let saved: any = "";
let turns = 0;
let junk: any = null;
function coerce(): number {
  saved = "changed";
  turns++;
  for (let i = 0; i < 128; i++) junk = { x: i, text: "allocation-" + i };
  if (typeof gc === "function") gc();
  return 2;
}
function combine(a: any, b: any): any {
  return saved + (a + b);
}
const coercer: any = { valueOf: coerce };
for (let i = 0; i < 12; i++) {
  saved = "captured-" + i;
  console.log(combine(coercer, 3));
}
console.log(turns, saved, junk.x);
