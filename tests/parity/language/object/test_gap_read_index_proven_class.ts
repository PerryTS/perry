// Reads of class receivers born with the key whose fixed-offset proof is
// refused (a private element; a computed accessor), at most four birth keys.
// Each key reads its own value, so a read that matched the wrong key would
// print another slot.
class Priv {
  #tag = 7;
  constructor(v) { this.a = v; this.b = v + 100; this.c = v + 200; this.d = v + 300; }
  tag() { return this.#tag; }
}
const name = 'w' + 'ide';
class Comp {
  constructor(v) { this.p = v; this.q = v * 10; }
  get [name]() { return this.p + this.q; }
}
function readB(o) { return o.b; }
function readC(o) { return o.c; }
function readD(o) { return o.d; }
function readQ(o) { return o.q; }
let sb = 0, sc = 0, sd = 0, sq = 0, st = 0, sw = 0;
for (let i = 0; i < 50; i++) {
  const o = new Priv(i);
  sb += readB(o); sc += readC(o); sd += readD(o); st += o.tag();
  const c = new Comp(i);
  sq += readQ(c); sw += c.wide;
}
console.log('priv b', sb, 'c', sc, 'd', sd, 'tag', st);
console.log('comp q', sq, 'wide', sw);
const gone = new Priv(1);
delete gone.c;
console.log('deleted c', readC(gone), 'b', readB(gone), 'd', readD(gone));
Object.setPrototypeOf(gone, { c: 'proto' });
console.log('proto c', readC(gone));
const late = new Comp(2);
late.q = 'set';
console.log('store q', readQ(late), late.p);
