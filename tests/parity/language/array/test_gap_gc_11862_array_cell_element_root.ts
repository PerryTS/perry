// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=11862 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_VERIFY_EVACUATION=1
// #11862: a local read out of an array element must stay rooted. The pointer
// analysis took the element type from the array literal (`[0]`, `[undefined]`)
// and ignored the in-place element write, so the local holding the element got
// no shadow slot and kept a from-space address across the next collection.
// HIR builds that shape itself for a repeatable class declaration whose
// members name the class: its evaluation owner is a one-element array box.
function make(n: number) {
  class Item {
    static all: Item[] = [];
    v: number;
    constructor(v: number) { this.v = v + n; Item.all.push(this); }
  }
  return Item;
}
function churn(k: number): number {
  let s = 0;
  for (let i = 0; i < 50; i++) { const o = { a: i, b: "s" + i, c: [i, k] }; s += o.c.length; }
  return s;
}
function classes(): string {
  let ok = 0;
  for (let i = 0; i < 60; i++) {
    const C: any = make(i);
    new C(i);
    churn(i);
    if (C.all.length === 1 && C.all[0].v === 2 * i) ok++;
  }
  return "classes " + ok;
}
function cells(): string {
  let ok = 0;
  for (let i = 0; i < 60; i++) {
    const a: any[] = [0];
    a[0] = { v: i, s: "v" + i };
    const x: any = a[0];
    const b = [undefined];
    (b as any)[0] = { w: i };
    const y: any = b[0];
    churn(i);
    if (x.v === i && x.s === "v" + i && y.w === i) ok++;
  }
  return "cells " + ok;
}
console.log(classes());
console.log(cells());
