// Direct and detached entries share a body, including its fresh capture environment.
declare function gc(): void;
function collect() { if (typeof gc === "function") gc(); }
function make(label: string) {
  class Base {
    m() { return label; }
    static m() { return label; }
  }
  class Child extends Base {
    m() { const prefix = label; collect(); return prefix + "/" + super.m(); }
    static m() { const prefix = label; collect(); return prefix + "/" + super.m(); }
  }
  const instance = new Child();
  console.log("direct", instance.m(), Child.m());
  return { instance, method: instance.m, staticMethod: Child.m };
}
const a = make("A"), b = make("B");
for (let i = 0; i < 12; i++) {
  collect();
  console.log(a.instance.m(), b.instance.m(), a.method.call(a.instance),
              a.staticMethod(), b.staticMethod());
}
