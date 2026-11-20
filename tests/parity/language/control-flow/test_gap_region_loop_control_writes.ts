// A loop region's Number proof for a loop-carried local must see every write
// the region can observe, not only the body's. The condition and the update
// clause run between iterations, after the one preheader entry test, so a
// local they rewrite to a string or an object must not be added as a double
// (and a pointer must never be stored into a pointer-free F64 lane).
class P {
  x: number;
  constructor() {
    this.x = 0;
  }
}

function classUpdate(o: P, s: any): any {
  for (let i = 0; i < 3; i++, s = "a") {
    o.x = o.x + s;
  }
  return o.x;
}

function literalUpdate(o: { x: number }, s: any): any {
  for (let i = 0; i < 3; i++, s = "b") {
    o.x = o.x + s;
  }
  return o.x;
}

function gen(i: number): any {
  return i < 1 ? 5 : "g" + i;
}

function condFor(o: P, n: number): any {
  let s: any = 1;
  for (let i = 0; (s = gen(i)), i < n; i++) {
    o.x = o.x + s;
  }
  return o.x;
}

let k = 0;
function next(): any {
  k++;
  return k === 1 ? 2 : k < 4 ? "w" + k : null;
}

function condWhile(o: { x: number }): any {
  let s: any = 1;
  while ((s = next()) && o.x !== -1) {
    o.x = o.x + s;
  }
  return o.x;
}

function commaUpdate(o: P, s: any, t: any): any {
  for (let i = 0; i < 4; (i++, (s = t), (t = "c"))) {
    o.x = o.x + s;
  }
  return o.x;
}

function compareUpdate(o: P, s: any): number {
  let hits = 0;
  for (let i = 0; i < 3; i++, s = "9") {
    if (o.x < s) hits++;
    o.x = o.x + 1;
  }
  return hits;
}

function objUpdate(o: P, s: any): any {
  const keep = { tag: "live" };
  for (let i = 0; i < 3; i++, s = keep) {
    o.x = o.x + s;
  }
  return o.x;
}

function main(): void {
  console.log("classUpdate", classUpdate(new P(), 1));
  console.log("literalUpdate", literalUpdate({ x: 0 }, 1));
  console.log("condFor", condFor(new P(), 3));
  k = 0;
  console.log("condWhile", condWhile({ x: 0 }));
  console.log("commaUpdate", commaUpdate(new P(), 1, 2));
  console.log("compareUpdate", compareUpdate(new P(), 5));
  console.log("objUpdate", objUpdate(new P(), 1));
  const big: P[] = [];
  for (let r = 0; r < 200; r++) {
    const o = new P();
    classUpdate(o, 1);
    big.push(o);
  }
  let junk: string[] = [];
  for (let r = 0; r < 20000; r++) junk.push("j" + r);
  console.log("after-gc", big[0].x, big[199].x, junk.length);
}
main();
