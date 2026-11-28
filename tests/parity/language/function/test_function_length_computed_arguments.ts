// A computed method's name and length (instance and static, string and symbol keys), a
// function that reads arguments, and an anonymous arrow returned from a function.
const work = Symbol("work");
class K {
  [work](a: number, b: number) { return a + b; }
  ["str"](a: number) { return a; }
  static [work](a: number, b: number, c: number) { return a; }
}
function withArgs(a: number, b: number) { return arguments.length + a + b; }
function mk() { return (x: number) => x; }
const k: any = new K();
const S: any = K;
console.log(k[work].name, k[work].length, k.str.name, k.str.length, S[work].name, S[work].length);
console.log(withArgs.length, mk().name === "", mk().length);
