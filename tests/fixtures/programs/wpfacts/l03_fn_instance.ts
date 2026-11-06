// Function instances (`new f()` on a constructor function) with
// prototype-assigned methods, next to object literals whose keys match the
// instances' keys. A function instance is born with no class id (it has no
// class-table identity), so no class-id proof may cover it: a site whose
// binding mixes literals and function instances must read both correctly.
function Vec(this: any, x: number, y: number) {
  this.x = x;
  this.y = y;
}
Vec.prototype.len2 = function (this: any) {
  return this.x * this.x + this.y * this.y;
};
Vec.prototype.add = function (this: any, o: any) {
  return new (Vec as any)(this.x + o.x, this.y + o.y);
};
function gx(p: any): any {
  return p.x + p.y;
}
const vs: any[] = [];
for (let i = 0; i < 6; i++) vs.push(new (Vec as any)(i, i + 1));
let acc = new (Vec as any)(0, 0);
for (const v of vs) acc = acc.add(v);
console.log(acc.x, acc.y, acc.len2(), gx(acc));
console.log(gx({ x: 1, y: 2 }), gx(vs[3]), gx({ y: 5, x: 6 }));
function Named(this: any, n: string) {
  this.name = n;
}
Named.prototype = { greet(this: any) { return "hi " + this.name; } };
const nm = new (Named as any)("ann");
console.log(nm.greet(), nm instanceof (Named as any), Object.keys(nm).join(","));
