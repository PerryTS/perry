// A template's first and later evaluations own distinct prototype data.
class LoopK { v() { return "outer"; } }
const classes: any[] = [];
const objects: any[] = [];
for (let i = 0; i < 3; i++) {
  class LoopK {
    f = 10;
    static s = 20;
    v() { return "inner"; }
    get value() { return this.f; }
    set value(n: number) { this.f = n; }
  }
  (LoopK.prototype as any).w = "w" + i;
  const o = new LoopK();
  o.value = i + 30;
  classes.push(LoopK);
  objects.push(o);
  console.log("loop", o.v(), (o as any).w, o.f, o.value, LoopK.s);
}
classes[1].s = 99;
classes[1].prototype.w = "changed";
for (let i = 0; i < 3; i++) {
  console.log("saved", objects[i].w, classes[i].s,
    Object.getPrototypeOf(objects[i]) === classes[i].prototype,
    objects[i] instanceof classes[i], objects[i] instanceof classes[(i + 1) % 3]);
}
console.log("outer", new LoopK().v(), (new LoopK() as any).w,
  classes[0] === classes[1], classes[0].prototype === classes[1].prototype);

// Instance initializers retain their loop evaluation even when run later.
const initialized: any[] = [];
for (let i = 0; i < 3; i++) {
  class LoopK {
    field = i;
    static value = i;
  }
  console.log("initialize", new LoopK().field, LoopK.value);
  initialized.push(LoopK);
}
for (let i = 0; i < 3; i++) {
  const C = initialized[i];
  console.log("later", new C().field, C.value);
}
