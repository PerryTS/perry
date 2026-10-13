// #12466: an instance's explicit [[Prototype]] decides its methods, before
// its nominal class.
class Walker {
  n = 0;
  step(): number { this.n += 1; return this.n; }
}
const xs: Walker[] = [new Walker(), new Walker(), new Walker()];
function run(): number {
  let total = 0;
  for (let i = 0; i < xs.length; i++) total += xs[i].step();
  return total;
}
console.log(run());
Object.setPrototypeOf(xs[1], { step(): number { return 100; } });
console.log(run());
const proto: any = Walker.prototype;
proto.step = function (): number { return 1000; };
console.log(run());
console.log(xs[0] instanceof Walker, xs[1] instanceof Walker);
