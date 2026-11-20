// A RegExp subclass as the split separator: @@split reads @@species from the
// subclass constructor (inherited from RegExp through the constructor's
// [[Prototype]]), constructs the splitter through the subclass and calls the
// subclass exec at every position.
class R extends RegExp {
  exec(s: string): any {
    console.log("exec", this instanceof R, this.lastIndex, this.flags);
    if (this.lastIndex !== 1) return null;
    this.lastIndex = 2;
    const r: any = ["b", { v: 7 }];
    r.index = 1;
    return r;
  }
}
function f(s: string, re: RegExp): any[] {
  return s.split(re);
}
const parts = f("abc", new R("b"));
console.log(JSON.stringify(parts), parts[1].v);
console.log(JSON.stringify(f("abc", new R("b", "g"))));
console.log(JSON.stringify("abc".split(new R("b"), 2)));
// Two levels of subclass: the species is the nearest constructor.
class S extends R {}
const deep = f("abc", new S("b"));
console.log(JSON.stringify(deep), deep[1].v);
