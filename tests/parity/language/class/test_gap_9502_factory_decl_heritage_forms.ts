// #9502: dynamic heritage includes bundled member access, aliases and shadows.
class First { kind() { return "first"; } }
class Second { kind() { return "second"; } }
function fromMember(mod: any): any {
  class Member extends mod.Base {}
  return Member;
}
function fromAlias(P: any): any {
  const Parent = P;
  class Aliased extends Parent {}
  return Aliased;
}
function fromShadow(First: any): any {
  class Shadowed extends First {}
  return Shadowed;
}
function check(label: string, factory: any): void {
  const One = factory(First);
  const Two = factory(Second);
  console.log(label + " " + (One !== Two) + " " + (Object.getPrototypeOf(One) === First)
    + " " + (Object.getPrototypeOf(Two) === Second));
  console.log(new One().kind() + " " + new Two().kind());
}
check("member", (P: any) => fromMember({ Base: P }));
check("alias", fromAlias);
check("shadow", fromShadow);

// A direct new-expression inside the factory must use the fresh local binding.
function localNew(P: any): any {
  class Local extends P {}
  return new Local();
}
const I1 = localNew(First);
const I2 = localNew(Second);
console.log("local new " + I1.kind() + " " + I2.kind());

// Static state must be initialized on each class object, even without captures
// in instance members. A static block still runs once per evaluation.
let blocks = 0;
function withStatics(P: any, tag: string): any {
  class Stateful extends P {
    static tag = tag;
    static count = 0;
    static { blocks++; }
  }
  return Stateful;
}
const S1 = withStatics(First, "one");
const S2 = withStatics(Second, "two");
S1.count++;
console.log("statics " + (S1 !== S2) + " " + S1.tag + " " + S2.tag
  + " " + S1.count + " " + S2.count + " " + blocks);
console.log("static parents " + (Object.getPrototypeOf(S1) === First)
  + " " + (Object.getPrototypeOf(S2) === Second));
console.log("static methods " + new S1().kind() + " " + new S2().kind());
