// Issue #838 — JS-classic prototype-method dispatch.
// dayjs / chalk / a long tail of npm packages still attach instance
// methods via `Class.prototype.<name> = fn` outside the ES6 class
// body. Pre-fix the assignment was a silent no-op at runtime.

// 1. Direct shape: `<Class>.prototype.<name> = <fn>`
class Dayjs {
  d: string;
  constructor(s: string) { this.d = s; }
}
(Dayjs.prototype as any).format = function(this: any) { return "fmt:" + this.d; };
const a = new Dayjs("2024-01-02");
console.log("a.format type:", typeof (a as any).format);
console.log("a.format():", (a as any).format());

// 2. Aliased shape: `let p = <Class>.prototype; p.<name> = <fn>`
// (the dayjs minified bundle uses this — `var m = M.prototype; m.parse = …`)
class M {
  s: string;
  constructor(s: string) { this.s = s; }
}
const p: any = M.prototype;
p.parse = function(this: any) { return "parsed:" + this.s; };
const b = new M("hello");
console.log("b.parse type:", typeof (b as any).parse);
console.log("b.parse():", (b as any).parse());

// 3. Multiple methods on the same prototype + `this`-aware composition
class Counter {
  n: number;
  constructor(n: number) { this.n = n; }
}
(Counter.prototype as any).inc = function(this: any) { this.n += 1; return this; };
(Counter.prototype as any).val = function(this: any) { return this.n; };
const c = new Counter(0);
(c as any).inc();
(c as any).inc();
(c as any).inc();
console.log("c.val():", (c as any).val());
