// WPS3 class D: a callee reached from the constructor adds a key to `this` between the
// constructor's own stores; the instance's key order is the insertion order, callee keys
// included. The same callee on an already-born instance appends after every constructor key.
function show(tag: string, o: any) {
  console.log(tag, JSON.stringify(Object.keys(o)), JSON.stringify(o));
}
class Ooo {
  [k: string]: any;
  constructor() {
    this.a = 1;
    this.b = (this.d = "early", 2);
    this.c = touch(this);
    this.d = "late";
  }
}
class Later {
  [k: string]: any;
  constructor() {
    this.a = 1;
    this.b = (this.d = "early", 2);
    this.c = 3;
    this.d = "late";
  }
}
function touch(o: any) { o.e = "extra"; return Object.keys(o).length; }
for (let i = 0; i < 3; i++) show("during" + i, new Ooo());
for (let i = 0; i < 2; i++) {
  const l = new Later();
  touch(l);
  show("after" + i, l);
}
