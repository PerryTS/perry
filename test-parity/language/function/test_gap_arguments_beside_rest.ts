// `arguments` in a function with a rest parameter is the arguments object:
// it holds every argument, and code handed it can change those objects.
// Such a function has an unmapped `arguments`: writing arguments[0] does
// not change the parameter, and the rest array is its own array.
class A {
  x: any = 1;
  y: any = "a";
}

function change(o: any): void {
  o[0].x = "changed";
  o[1].y = 7;
  o[0] = null;
}

function g(p: any, ...r: any[]): number {
  change(arguments);
  return arguments.length + (p === null ? 100 : 0) + (r[0] === null ? 1000 : 0);
}

function h(...r: any[]): number {
  const o: any = arguments;
  o[0].x = 42;
  return r.length;
}

const a = new A();
const b = new A();
console.log(g(a, b, 3));
console.log(a.x, typeof a.x, b.y, typeof b.y);
console.log(h(a), a.x + 1);
