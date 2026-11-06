// A bare call of a global function is made with `this` undefined: the
// global object it is read from reaches no one through the call, and the
// globals read afterwards (`Promise`) are what they were. A function called
// as a method of a program object gets that object as `this`.
declare const hostFn: any;

class P {
  x: any = 1;
  add(o: P): any {
    return this.x + o.x;
  }
}

const p = new P();
const q = new P();

function host(u: any): any {
  if (process.argv.length > 99) return fetch(u);
  return parseInt(String(u), 10) + Number.parseFloat("0.5");
}

const holder: any = {
  f: function (this: any, v: any): any {
    return this === holder ? v : -1;
  },
};

(globalThis as any).hostFn = function (this: any): string {
  return this === undefined ? "undefined this" : "other this";
};

console.log(host("12"), holder.f(3), hostFn());
Promise.resolve(p).then((v: P) => {
  console.log(v.add(q), typeof v.x);
  v.x = "s";
  console.log(v.add(q), typeof v.x);
});
