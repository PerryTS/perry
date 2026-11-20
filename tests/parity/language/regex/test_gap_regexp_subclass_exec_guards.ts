// Guards: exec/test/match/replace/search already run a subclass exec, a written
// RegExp.prototype.exec already drives @@split, and an own exec on a plain
// RegExp is not the splitter's (species constructs a fresh RegExp).
let calls = 0;
class Tag extends RegExp {
  exec(s: string): any {
    calls++;
    const i = s.indexOf("b", this.lastIndex);
    if (i < 0) {
      this.lastIndex = 0;
      return null;
    }
    if (this.global || this.sticky) this.lastIndex = i + 1;
    const r: any = ["b", "B"];
    r.index = i;
    r.input = s;
    return r;
  }
}
console.log("exec", JSON.stringify(new Tag("b").exec("abcb")), calls);
console.log("test", new Tag("b").test("abc"), new Tag("b").test("xyz"), calls);
console.log("match", JSON.stringify("abcb".match(new Tag("b"))), JSON.stringify("abcb".match(new Tag("b", "g"))), calls);
console.log("replace", "abcb".replace(new Tag("b"), "[$1]"), "abcb".replace(new Tag("b", "g"), "[$1]"), calls);
console.log("search", "abcb".search(new Tag("b")), "xyz".search(new Tag("b")), calls);
// A nonsense exec result still flows through exec and @@match.
class Objects extends RegExp {
  exec(s: string): any {
    const r: any = [{ v: 1 }, { v: 2 }];
    r.index = 0;
    return r;
  }
}
const o = new Objects("q");
console.log("objects", (o.exec("q") as any)[1].v, ("q".match(o) as any)[0].v);
// A written RegExp.prototype.exec drives @@split.
const orig = RegExp.prototype.exec;
try { RegExp.prototype.exec = function (this: RegExp, s: string): any {
  if (this.lastIndex !== 1) return null;
  this.lastIndex = 2;
  const r: any = ["b", { v: 7 }];
  r.index = 1;
  return r;
}; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
const parts: any[] = "abc".split(/b/);
console.log("written proto exec", parts.length, parts[1].v);
try { RegExp.prototype.exec = orig; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
// An own exec on a plain RegExp is not the splitter's.
const own: any = /b/;
own.exec = function () {
  throw new Error("own exec must not run for split");
};
console.log("own exec", JSON.stringify("abc".split(own)));
