// A generator function expression in a class field initializer (`static f =
// function* () {}`, `inst = function* () {}`, or one passed to a call inside
// the initializer) was never lowered to a generator, so calling it returned
// the first yielded value or undefined instead of an iterator. The generator
// transform visited method, constructor and accessor bodies but not field
// initializers.

function drive(it: any): string {
  const out: string[] = [];
  for (let r = it.next(); ; r = it.next()) {
    out.push((r.done ? "done:" : "yield:") + String(r.value));
    if (r.done) break;
  }
  return out.join(",");
}

// Mimics Effect's `fnUntraced(function* ...)`: wrap a generator body and drive
// it with the call-time `this` and arguments.
function wrap(body: any): any {
  return function (this: any, ...args: any[]) {
    const it = body.apply(this, args);
    const seen: any[] = [];
    for (let r = it.next(); ; r = it.next()) {
      if (r.done) return { seen, result: r.value };
      seen.push(r.value);
    }
  };
}

class A {
  static f1 = function* (x: number): any {
    yield x;
    return 2;
  };
  static f0 = function* (): any {
    yield "a";
    yield "b";
  };
  static s = wrap(function* (this: any, a: number, b: number): any {
    yield a;
    yield b;
    yield this === A ? "static-this" : "wrong-this";
    return a + b;
  });

  inst = function* (x: number): any {
    yield x * 10;
    return "inst-done";
  };
  base = 5;
  w = wrap(function* (this: any, n: number): any {
    yield n;
    yield this.base;
    return n + this.base;
  });
}

const g: any = A.f1(7);
console.log(typeof g, typeof g?.next);
console.log(drive(g));
console.log(drive(A.f0()));
console.log(drive(A.f1.call(null, 3)));
console.log(drive(A.f1.apply(null, [4])));
console.log(JSON.stringify(A.s(1, 2)));
console.log(JSON.stringify(A.s.call(A, 3, 4)));

const a = new A();
console.log(typeof a.inst(1)?.next);
console.log(drive(a.inst(4)));
console.log(JSON.stringify(a.w(2)));

const E = class {
  static gen = function* (): any {
    yield 1;
    yield 2;
    return 3;
  };
};
console.log(drive(E.gen()));
