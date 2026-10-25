class A { layer = "first"; deps = []; }
class B { deps = []; layer = "second"; }
function factory(input: any) { return () => input.layer; }
function indirect(input: any) { return factory(input); }
let first = () => "unset";
const meter = { n: 0 };
for (let i = 0; i < 2; i++) {
  meter.n = meter.n + 1;
  first = factory(new A());
}
const second = indirect(new B());
console.log(first(), second(), meter.n);
