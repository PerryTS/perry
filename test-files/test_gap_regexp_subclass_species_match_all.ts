// matchAll on a RegExp subclass: the matcher is constructed through the
// subclass @@species and the subclass exec drives the iteration.
class R extends RegExp {
  exec(s: string): any {
    console.log("exec", this instanceof R, this.lastIndex);
    if (this.lastIndex >= 3) return null;
    const i = this.lastIndex;
    this.lastIndex = i + 1;
    const r: any = [s[i]];
    r.index = i;
    return r;
  }
}
const all: any[] = [...("xyz".matchAll(new R("q", "g")))];
console.log(all.length, JSON.stringify(all.map((m: any) => m[0] + "@" + m.index)));
class Never extends RegExp {
  exec(): any {
    return null;
  }
}
console.log([...("abc".matchAll(new Never("b", "g")))].length);
